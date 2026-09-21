//! The CDP driver. Each public method mirrors a step the reference tool performs
//! against the live editor; the private helpers wrap the editor's flaky
//! JavaScript timing (loading spinner, lazily-defined globals, upload status).

use std::collections::BTreeSet;
use std::future::Future;
use std::path::Path;
use std::time::Duration;

use chromiumoxide::cdp::browser_protocol::dom::SetFileInputFilesParams;
use chromiumoxide::cdp::browser_protocol::page::{
    EventJavascriptDialogOpening, HandleJavaScriptDialogParams,
};
use chromiumoxide::error::CdpError;
use chromiumoxide::js::Evaluation;
use chromiumoxide::{Browser, BrowserConfig, Element, Page};
use futures::StreamExt;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Instant};

use autofill_core::Details;

use crate::site::{self, Site};
use crate::{js, Error, Result};

const SPINNER_TIMEOUT: Duration = Duration::from_secs(30);
const DEFINE_TIMEOUT: Duration = Duration::from_secs(10);
const PRODUCT_SETTLE_TIMEOUT: Duration = Duration::from_secs(30);
const PRODUCT_SETTLE_STABLE_FOR: Duration = Duration::from_millis(500);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_millis(100);
const CONTEXT_RETRY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TRIES: u32 = 3;

/// The mode actually selected by the editor for a face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMode {
    Same,
    Different,
}

pub struct Driver {
    page: Page,
    site: &'static Site,
    // Held so the browser connection and its event pump stay alive for the
    // driver's lifetime; `close` (or drop) tears both down.
    browser: Browser,
    handler: JoinHandle<()>,
}

impl Driver {
    /// Launch a visible Chrome and open the target site's starting page.
    pub async fn launch(chrome_path: Option<&str>) -> Result<Driver> {
        let site = &site::MAKE_PLAYING_CARDS;

        let mut builder = BrowserConfig::builder()
            .with_head()
            .window_size(1200, 900)
            .viewport(None);
        if let Some(path) = chrome_path {
            builder = builder.chrome_executable(path);
        }
        let config = builder
            .build()
            .map_err(Error::Site)
            .map_err(|error| Error::stage("startup/browser-launch", error))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|error| Error::stage("startup/browser-launch", error.into()))?;
        let handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser
            .new_page(site.starting_url())
            .await
            .map_err(|error| Error::stage("startup/new-page", error.into()))?;
        auto_accept_dialogs(&page)
            .await
            .map_err(|error| Error::stage("startup/dialog-listener", error))?;

        Ok(Driver {
            page,
            site,
            browser,
            handler: handler_task,
        })
    }

    /// Close the browser and stop its event pump. Kills the child Chrome process.
    pub async fn close(mut self) {
        let _ = self.browser.close().await;
        self.handler.abort();
    }

    // region authentication

    pub async fn is_authenticated(&self) -> Result<bool> {
        let expr = format!(
            "document.querySelectorAll(\"a[href='{}']\").length > 0",
            self.site.logout_url()
        );
        self.eval_bool(expr).await
    }

    /// Send the visible browser to the login page and surface it to the user.
    pub async fn prompt_sign_in(&self) -> Result<()> {
        self.page
            .goto(self.site.login_url())
            .await
            .map_err(|error| Error::stage("auth/login-navigation", error.into()))?;
        self.page
            .bring_to_front()
            .await
            .map_err(|error| Error::stage("auth/bring-to-front", error.into()))?;
        Ok(())
    }

    pub async fn wait_until_authenticated(&self) -> Result<()> {
        let deadline = Instant::now() + SIGN_IN_TIMEOUT;
        while !self.is_authenticated().await? {
            if Instant::now() >= deadline {
                return Err(Error::Site("timed out waiting for you to sign in".into()));
            }
            sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }

    // endregion

    // region project setup

    pub async fn goto_start(&self) -> Result<()> {
        self.page.goto(self.site.starting_url()).await?;
        self.wait_for_idle().await?;
        self.wait_for_product_controls(false, false).await?;
        Ok(())
    }

    /// Choose cardstock, the smallest fitting bracket, and (optionally) foil.
    pub async fn define_project(&self, details: &Details) -> Result<()> {
        let stock = details.stock.label();
        if !self
            .eval_bool(js::select_by_text(self.site.cardstock_dropdown_id, stock))
            .await?
        {
            return Err(Error::Site(format!(
                "cardstock '{stock}' is not offered by this site"
            )));
        }
        // Stock changes rebuild the quantity brackets, so do not attempt the
        // bracket selection until both stock and quantity have settled.
        self.wait_for_product_controls(true, false).await?;
        if !self
            .eval_bool(js::select_smallest_bracket(
                self.site.quantity_dropdown_id,
                details.quantity,
            ))
            .await?
        {
            return Err(Error::Site(format!(
                "no bracket fits {} cards",
                details.quantity
            )));
        }
        self.wait_for_product_controls(true, false).await?;
        let requested_foil = details.foil && self.site.supports_foil;
        // The print-type select is not needed for ordinary orders, and some
        // product variants never expose it. Require it only to select foil.
        if requested_foil {
            self.wait_for_product_controls(true, true).await?;
        }
        if requested_foil
            && !self
                .eval_bool(js::select_by_value(
                    self.site.print_type_dropdown_id,
                    self.site.foil_option_value,
                ))
                .await?
        {
            return Err(Error::Site("foil is not offered by this site".into()));
        }
        if requested_foil {
            self.wait_for_product_controls(true, true).await?;
        }

        // Each selection can asynchronously replace all of the controls. Check
        // the settled, read-only state so a rebuild cannot silently discard an
        // earlier stock, bracket, or requested foil choice. Non-foil orders
        // deliberately retain the site's/source's normal print-type default.
        if !self
            .eval_bool(js::product_configuration_matches(
                self.site.cardstock_dropdown_id,
                stock,
                self.site.quantity_dropdown_id,
                details.quantity,
                self.site.print_type_dropdown_id,
                requested_foil.then_some(self.site.foil_option_value),
            ))
            .await?
        {
            return Err(Error::Site(
                "product configurator did not retain the requested stock, quantity bracket, or foil"
                    .into(),
            ));
        }
        Ok(())
    }

    pub async fn page_to_fronts(&self, quantity: u32) -> Result<()> {
        self.wait_until_defined("doPersonalize", false)
            .await
            .map_err(|error| Error::stage("fronts/personalize", error))?;
        let accept = self.site.url(self.site.accept_settings_route);
        match self.run(js::do_personalize(&accept)).await {
            Ok(()) => {}
            // The navigation may have begun after doPersonalize ran. Do not
            // replay this mutation; the editor destination must prove success.
            Err(error) if is_context_destroyed_error(&error) => {}
            Err(error) => return Err(Error::stage("fronts/personalize", error)),
        }
        self.wait_for_idle()
            .await
            .map_err(|error| Error::stage("fronts/wait-editor", error))?;
        self.wait_for_editor_quantity()
            .await
            .map_err(|error| Error::stage("fronts/wait-editor", error))?;

        self.wait_until_defined("setMode", true)
            .await
            .map_err(|error| Error::stage("fronts/mode", error))?;
        self.wait_until_defined("oRenderFeature", true)
            .await
            .map_err(|error| Error::stage("fronts/mode", error))?;
        self.run(js::set_card_quantity(quantity))
            .await
            .map_err(|error| Error::stage("fronts/quantity", error))?;
        self.run(js::different_images())
            .await
            .map_err(|error| Error::stage("fronts/mode", error))?;
        self.wait_for_idle()
            .await
            .map_err(|error| Error::stage("fronts/mode", error))?;
        Ok(())
    }

    /// Navigate to backs and select the editor mode appropriate for this order.
    /// A single common back can use Same only when the editor is still displaying
    /// its one-slot layout. Before mode configuration, an absent layout or slot
    /// probe exception is the editor's expected initial one-image state.
    pub async fn page_to_backs(
        &self,
        common_back_eligible: bool,
        quantity: u32,
    ) -> Result<ImageMode> {
        self.next_step("backs/next-fronts").await?;
        self.wait_for_idle()
            .await
            .map_err(|error| Error::stage("backs/next-fronts", error))?;
        self.wait_for_front_text_ready().await?;

        // The text editor can show an instructional popup over its next button.
        // Close it only after this is demonstrably the ready text-editor page.
        if let Ok(close) = self.page.find_element("#closeBtn").await {
            let _ = close.click().await;
        }

        self.advance_front_text_to_backs().await?;

        // This deliberately precedes renderDesignCount, but must not wait for
        // PageLayout or slot zero: either may be absent in the initial
        // single-image layout. The probe catches those in-page exceptions while
        // CDP/evaluation failures still propagate.
        let same_layout = initial_layout_is_single(
            quantity,
            self.eval_bool_during_poll(
                js::INITIAL_LAYOUT_HAS_SLOT_ONE,
                Instant::now() + DEFINE_TIMEOUT,
            )
            .await?,
        );

        self.acquire_back_chooser().await?;

        let mode = select_back_mode(common_back_eligible, same_layout);
        // The generated mutation re-checks the visible chooser, expected MPC
        // pathname, and its mode globals immediately before setMode. Never
        // suppress or replay an exception from this editor mutation.
        match mode {
            ImageMode::Same => self
                .run(js::same_back_images())
                .await
                .map_err(|error| Error::stage("backs/set-mode", error))?,
            ImageMode::Different => self
                .run(js::different_back_images())
                .await
                .map_err(|error| Error::stage("backs/set-mode", error))?,
        };
        self.wait_for_idle()
            .await
            .map_err(|error| Error::stage("backs/set-mode", error))?;
        // Once the mode has been configured, slot zero is required for the
        // subsequent insertion phase.
        self.wait_for_slot(0)
            .await
            .map_err(|error| Error::stage("backs/wait-slots", error))?;
        Ok(mode)
    }

    pub async fn page_to_review(&self) -> Result<()> {
        self.next_step("review/next-design").await?;
        self.next_step("review/next-review").await
    }

    // endregion

    // region uploading

    /// Upload `image_path` (if not already uploaded) and drop it into each of its
    /// `slots`. Idempotent: slots already holding this image are left alone.
    pub async fn upload_and_insert(&self, image_path: &Path, slots: &BTreeSet<u32>) -> Result<()> {
        let bytes = tokio::fs::read(image_path).await?;
        let pid = autofill_core::pid(&bytes);

        if !self.is_pid_uploaded(&pid).await? {
            self.upload_with_retries(image_path).await?;
        }

        self.wait_until_defined("PageLayout.prototype.applyDragPhoto", false)
            .await?;
        for &slot in slots {
            self.wait_for_slot(slot).await?;
            if self.eval_string(js::pid_in_slot(slot)).await? == pid {
                continue;
            }
            self.insert_with_retries(slot, &pid).await?;
        }
        Ok(())
    }

    /// Upload a file, verifying the project's image count actually grew and
    /// retrying if it didn't — the site drops uploads intermittently.
    async fn upload_with_retries(&self, path: &Path) -> Result<()> {
        let before = self.eval_u32(js::UPLOADED_IMAGE_COUNT).await?;
        for attempt in 1..=MAX_TRIES {
            self.wait_while_uploading().await?;
            self.upload_file(path).await?;
            // Give the site a beat to register the upload before polling status,
            // matching the reference tool's post-send settle.
            sleep(Duration::from_secs(1)).await;
            self.wait_while_uploading().await?;

            if self.eval_u32(js::UPLOADED_IMAGE_COUNT).await? > before {
                return Ok(());
            }
            tracing::warn!(
                "upload attempt {attempt} for {} did not register",
                path.display()
            );
        }
        Err(Error::Site(format!(
            "failed to upload {} after {MAX_TRIES} attempts",
            path.display()
        )))
    }

    /// Insert `pid` into `slot`, re-issuing if a spinner-triggered page reload
    /// interrupts the drop.
    async fn insert_with_retries(&self, slot: u32, pid: &str) -> Result<()> {
        for attempt in 1..=MAX_TRIES {
            let inserted = async {
                self.wait_until_defined("PageLayout.prototype.applyDragPhoto", false)
                    .await?;
                self.wait_for_slot(slot).await?;
                self.run(js::insert_into_slot(slot, pid)).await?;
                Ok(!self.wait_for_idle().await?
                    && self.eval_string(js::pid_in_slot(slot)).await? == pid)
            }
            .await;

            match inserted {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    tracing::warn!("insertion attempt {attempt} for slot {slot} did not persist");
                }
                // A failed evaluation after a mutation is ambiguous. The
                // evaluation helper only retries the CDP rejection that proves
                // the script did not run; every other error must surface.
                Err(error) => return Err(error),
            }
        }
        Err(Error::Site(format!(
            "failed to insert image into required slot {slot} after {MAX_TRIES} attempts"
        )))
    }

    async fn upload_file(&self, path: &Path) -> Result<()> {
        let input = self.find_element("#uploadId").await?;
        let params = SetFileInputFilesParams::builder()
            .files(vec![path.to_string_lossy().into_owned()])
            .backend_node_id(input.backend_node_id)
            .build()
            .map_err(Error::Site)?;
        self.page.execute(params).await?;
        Ok(())
    }

    async fn is_pid_uploaded(&self, pid: &str) -> Result<bool> {
        let list = self.eval_string(js::UPLOADED_PIDS).await?;
        Ok(list.split(';').any(|uploaded| uploaded == pid))
    }

    async fn wait_while_uploading(&self) -> Result<()> {
        let deadline = Instant::now() + UPLOAD_TIMEOUT;
        while self
            .eval_bool_during_poll(js::UPLOAD_STATUS_IS_UPLOADING, deadline)
            .await?
        {
            if Instant::now() >= deadline {
                return Err(Error::Site(
                    "timed out waiting for image upload to finish".into(),
                ));
            }
            sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    // endregion

    // region editor timing helpers

    async fn wait_for_front_text_ready(&self) -> Result<()> {
        wait_for_editor_state_with(
            "front-text/wait-ready",
            Instant::now() + DEFINE_TIMEOUT,
            POLL,
            |deadline| async move { self.eval_string_until(js::FRONT_TEXT_STATE, deadline).await },
        )
        .await
    }

    async fn advance_front_text_to_backs(&self) -> Result<()> {
        advance_front_text_to_backs_with(
            Instant::now() + DEFINE_TIMEOUT,
            Instant::now() + DEFINE_TIMEOUT,
            POLL,
            |deadline| async move { self.eval_string_until(js::FRONT_TEXT_STATE, deadline).await },
            || async move { self.run(js::advance_front_text()).await },
            |deadline| async move {
                self.eval_string_until(js::BACKS_DESTINATION_STATE, deadline)
                    .await
            },
        )
        .await
    }

    async fn next_step(&self, stage: &'static str) -> Result<()> {
        self.wait_for_idle()
            .await
            .map_err(|error| Error::stage(stage, error))?;
        self.wait_until_defined("oDesign.setNextStep", false)
            .await
            .map_err(|error| Error::stage(stage, error))?;
        self.run(js::NEXT_STEP)
            .await
            .map_err(|error| Error::stage(stage, error))
    }

    /// Find an element, retrying briefly to mimic Selenium's implicit wait — some
    /// controls (e.g. the upload input) appear a beat after the page settles.
    async fn find_element(&self, selector: &str) -> Result<Element> {
        for _ in 0..25 {
            if let Ok(element) = self.page.find_element(selector).await {
                return Ok(element);
            }
            sleep(Duration::from_millis(200)).await;
        }
        Err(Error::Site(format!(
            "timed out waiting for required editor element `{selector}`"
        )))
    }

    /// Wait for the loading spinner to disappear, reloading once if it hangs.
    /// Returns whether a reload was forced, so callers can retry interrupted work.
    async fn wait_for_idle(&self) -> Result<bool> {
        wait_for_idle_with(
            SPINNER_TIMEOUT,
            POLL,
            |deadline| async move {
                self.eval_bool_during_poll(js::LOADING_SPINNER_VISIBLE, deadline)
                    .await
            },
            || async move {
                self.page.reload().await?;
                Ok(())
            },
        )
        .await
    }

    /// Wait until the configurator has stopped rebuilding its controls. Require
    /// a short stable interval so an event handler that adds `.productPageLoading`
    /// just after a selection cannot be mistaken for a settled page.
    async fn wait_for_product_controls(
        &self,
        require_quantity: bool,
        require_print_type: bool,
    ) -> Result<()> {
        let deadline = Instant::now() + PRODUCT_SETTLE_TIMEOUT;
        let readiness = js::product_controls_ready(
            self.site.cardstock_dropdown_id,
            require_quantity.then_some(self.site.quantity_dropdown_id),
            require_print_type.then_some(self.site.print_type_dropdown_id),
        );
        let mut ready_since = None;
        loop {
            match self.eval_bool_until(readiness.as_str(), deadline).await {
                Ok(true) => {
                    let since = *ready_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= PRODUCT_SETTLE_STABLE_FOR {
                        return Ok(());
                    }
                }
                Ok(false) => ready_since = None,
                // A navigation replaces the observed DOM. Do not let the
                // stable-ready interval include that unobserved gap.
                Err(error) if is_context_turnover_error(&error) => ready_since = None,
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(Error::Site(
                    "timed out waiting for product configurator controls".into(),
                ));
            }
            sleep(POLL).await;
        }
    }

    async fn wait_for_editor_quantity(&self) -> Result<()> {
        let deadline = Instant::now() + DEFINE_TIMEOUT;
        while !self
            .eval_bool_during_poll(js::EDITOR_QUANTITY_READY, deadline)
            .await?
        {
            if Instant::now() >= deadline {
                return Err(Error::Site(
                    "timed out waiting for editor iframe quantity input".into(),
                ));
            }
            sleep(POLL).await;
        }
        Ok(())
    }

    /// Some editor globals load lazily; fail descriptively if one never appears.
    async fn wait_until_defined(&self, symbol: &str, in_frame: bool) -> Result<()> {
        let deadline = Instant::now() + DEFINE_TIMEOUT;
        let defined = js::is_defined(symbol, in_frame);
        while !self.eval_bool_during_poll(&defined, deadline).await? {
            if Instant::now() >= deadline {
                return Err(Error::Site(format!(
                    "timed out waiting for editor function `{symbol}`"
                )));
            }
            sleep(Duration::from_millis(500)).await;
        }
        Ok(())
    }

    /// Acquire the MPC back-mode chooser without invoking the legacy render
    /// helper against an uninitialized layout. A visible expected chooser is
    /// either ready or loading; it is never reopened. We issue at most one
    /// render mutation after the read-only state reports an initialized layout.
    async fn acquire_back_chooser(&self) -> Result<()> {
        acquire_back_chooser_with(
            Instant::now() + DEFINE_TIMEOUT,
            POLL,
            |deadline| async move {
                self.eval_string_until(js::BACK_CHOOSER_STATE, deadline)
                    .await
                    .map_err(|error| Error::stage("backs/wait-mode-entry", error))
            },
            || async move {
                // `open_back_chooser` repeats its prerequisite immediately
                // before renderDesignCount. Its mutation is not retried.
                self.run(js::open_back_chooser())
                    .await
                    .map_err(|error| Error::stage("backs/open-mode", error))
            },
        )
        .await
    }

    async fn wait_for_slot(&self, slot: u32) -> Result<()> {
        wait_for_editor_state_with(
            "editor-slot/wait-ready",
            Instant::now() + DEFINE_TIMEOUT,
            POLL,
            |deadline| async move { self.eval_string_until(js::slot_state(slot), deadline).await },
        )
        .await
    }

    /// A navigation can replace Chrome's execution context between two polling
    /// evaluations. Only that narrow, read-only case is retried; mutating calls
    /// still surface their errors to avoid duplicating an editor action.
    async fn eval_bool_during_poll(&self, js: &str, deadline: Instant) -> Result<bool> {
        loop {
            match self.eval_bool_until(js, deadline).await {
                Ok(value) => return Ok(value),
                Err(error) if is_context_turnover_error(&error) && Instant::now() < deadline => {
                    sleep(POLL).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn evaluate(
        &self,
        js: impl Into<Evaluation>,
    ) -> Result<chromiumoxide::js::EvaluationResult> {
        let js = js.into();
        // The ordinary first evaluation uses chromiumoxide's normal CDP
        // timeout. A short recovery window starts only after the one rejection
        // which proves Chrome did not execute the script.
        recover_missing_context(CONTEXT_RETRY_TIMEOUT, POLL, || async {
            self.page.evaluate(js.clone()).await.map_err(Error::from)
        })
        .await
    }

    /// Bound a read-only evaluation and all of its context retries by its
    /// caller's polling deadline rather than granting each attempt a new budget.
    async fn evaluate_until(
        &self,
        js: impl Into<Evaluation>,
        deadline: Instant,
    ) -> Result<chromiumoxide::js::EvaluationResult> {
        let js = js.into();
        retry_missing_context(deadline, POLL, || async {
            self.page.evaluate(js.clone()).await.map_err(Error::from)
        })
        .await
    }

    async fn run(&self, js: impl Into<Evaluation>) -> Result<()> {
        self.evaluate(js).await?;
        Ok(())
    }

    async fn eval_bool(&self, js: impl Into<Evaluation>) -> Result<bool> {
        self.evaluate(js)
            .await?
            .into_value()
            .map_err(|error| Error::Site(format!("editor returned invalid boolean: {error}")))
    }

    async fn eval_bool_until(&self, js: impl Into<Evaluation>, deadline: Instant) -> Result<bool> {
        self.evaluate_until(js, deadline)
            .await?
            .into_value()
            .map_err(|error| Error::Site(format!("editor returned invalid boolean: {error}")))
    }

    async fn eval_string(&self, js: impl Into<Evaluation>) -> Result<String> {
        self.evaluate(js)
            .await?
            .into_value()
            .map_err(|error| Error::Site(format!("editor returned invalid string: {error}")))
    }

    async fn eval_string_until(
        &self,
        js: impl Into<Evaluation>,
        deadline: Instant,
    ) -> Result<String> {
        self.evaluate_until(js, deadline)
            .await?
            .into_value()
            .map_err(|error| Error::Site(format!("editor returned invalid string: {error}")))
    }

    async fn eval_u32(&self, js: impl Into<Evaluation>) -> Result<u32> {
        self.evaluate(js)
            .await?
            .into_value()
            .map_err(|error| Error::Site(format!("editor returned invalid number: {error}")))
    }

    // endregion
}

/// The initial layout is single-image when its second slot is absent. `quantity`
/// of one never needs the multi-card Same mode, regardless of editor layout.
fn initial_layout_is_single(quantity: u32, has_slot_one: bool) -> bool {
    quantity > 1 && !has_slot_one
}

fn select_back_mode(common_back_eligible: bool, same_layout: bool) -> ImageMode {
    if common_back_eligible && same_layout {
        ImageMode::Same
    } else {
        ImageMode::Different
    }
}

/// Poll a read-only transition state to readiness with a bounded, useful
/// diagnosis. The probe is deliberately responsible for reporting null or
/// incomplete editor state; it never repairs that state.
async fn wait_for_editor_state_with<R, RFut>(
    stage: &'static str,
    deadline: Instant,
    poll: Duration,
    mut read_state: R,
) -> Result<()>
where
    R: FnMut(Instant) -> RFut,
    RFut: Future<Output = Result<String>>,
{
    let mut last_state = None;
    loop {
        // The preceding poll sleep may have exhausted the shared evaluation
        // budget. Do not start a fresh probe after that point.
        if Instant::now() >= deadline {
            let state = last_state.as_deref().unwrap_or("unobserved");
            return Err(Error::Site(format!(
                "{stage}: timed out waiting for ready state (state: {state})"
            )));
        }
        match read_state(deadline).await {
            Ok(state) => {
                if state == "ready" {
                    return Ok(());
                }
                last_state = Some(state);
            }
            // A bounded evaluation can expire while this final probe is in
            // flight. It is the polling deadline, so retain the last observed
            // state in the diagnostic rather than returning a generic CDP timeout.
            Err(Error::Cdp(CdpError::Timeout)) if Instant::now() >= deadline => {
                let state = last_state.as_deref().unwrap_or("unobserved");
                return Err(Error::Site(format!(
                    "{stage}: timed out waiting for ready state (state: {state})"
                )));
            }
            // These errors only mean a read-only probe lost its execution
            // context. Unlike mutations, it is safe to observe again while
            // retaining the caller's original deadline.
            Err(error) if is_context_turnover_error(&error) => {}
            Err(error) => return Err(Error::stage(stage, error)),
        }
        sleep(poll).await;
    }
}

/// Advance the ready front text editor once, then require proof of the backs
/// destination. An execution-context destruction can happen after the editor
/// mutation ran, so it is resolved only by destination proof and never replayed.
async fn advance_front_text_to_backs_with<F, FFut, A, AFut, B, BFut>(
    front_deadline: Instant,
    backs_deadline: Instant,
    poll: Duration,
    read_front: F,
    mut advance: A,
    read_backs: B,
) -> Result<()>
where
    F: FnMut(Instant) -> FFut,
    FFut: Future<Output = Result<String>>,
    A: FnMut() -> AFut,
    AFut: Future<Output = Result<()>>,
    B: FnMut(Instant) -> BFut,
    BFut: Future<Output = Result<String>>,
{
    wait_for_editor_state_with("front-text/advance", front_deadline, poll, read_front).await?;
    match advance().await {
        Ok(()) => {}
        Err(error) if is_context_destroyed_error(&error) => {}
        Err(error) => return Err(Error::stage("front-text/advance", error)),
    }
    wait_for_editor_state_with("backs/wait-backs", backs_deadline, poll, read_backs).await
}

/// State-machine core for back chooser acquisition. Keeping the read and the
/// one permitted mutation injectable makes its no-reopen contract testable
/// without a browser.
async fn acquire_back_chooser_with<R, RFut, O, OFut>(
    deadline: Instant,
    poll: Duration,
    mut read_state: R,
    mut open: O,
) -> Result<()>
where
    R: FnMut(Instant) -> RFut,
    RFut: Future<Output = Result<String>>,
    O: FnMut() -> OFut,
    OFut: Future<Output = Result<()>>,
{
    let mut opened = false;
    loop {
        let state = read_state(deadline).await?;
        match state.as_str() {
            "ready" => return Ok(()),
            "openable" if !opened => {
                open().await?;
                opened = true;
            }
            // A visible loading chooser is deliberately not reopened.
            _ => {}
        }
        if Instant::now() >= deadline {
            return Err(Error::Site(format!(
                "timed out waiting for back image-mode chooser (state: {state})"
            )));
        }
        sleep(poll).await;
    }
}

/// Poll a spinner until it disappears, reloading if it remains visible for the
/// full timeout. Check the deadline before each new probe: the preceding sleep
/// may have consumed the remaining budget, in which case the reload recovery is
/// required instead of a bounded evaluation reporting a timeout.
async fn wait_for_idle_with<E, EFut, R, RFut>(
    timeout: Duration,
    poll: Duration,
    mut spinner_visible: E,
    mut reload: R,
) -> Result<bool>
where
    E: FnMut(Instant) -> EFut,
    EFut: Future<Output = Result<bool>>,
    R: FnMut() -> RFut,
    RFut: Future<Output = Result<()>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            reload().await?;
            return Ok(true);
        }
        if !spinner_visible(deadline).await? {
            return Ok(false);
        }
        sleep(poll).await;
    }
}

/// Run an ordinary evaluation with its normal CDP timeout. Only a structured
/// missing-context rejection starts the short, bounded recovery window.
async fn recover_missing_context<T, F, Fut>(
    recovery_timeout: Duration,
    retry_delay: Duration,
    mut evaluate: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    match evaluate().await {
        Ok(value) => Ok(value),
        Err(error) if is_context_not_found_error(&error) => {
            retry_missing_context(Instant::now() + recovery_timeout, retry_delay, evaluate).await
        }
        Err(error) => Err(error),
    }
}

/// Retry only the CDP rejection that proves evaluation never reached JavaScript.
/// In particular, an execution-context-destroyed error may arrive after a
/// mutation executed and is never replayed by this helper.
async fn retry_missing_context<T, F, Fut>(
    deadline: Instant,
    retry_delay: Duration,
    mut evaluate: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    loop {
        // Never issue an evaluation once its caller's budget is exhausted.
        if Instant::now() >= deadline {
            return Err(Error::Cdp(CdpError::Timeout));
        }
        match tokio::time::timeout_at(deadline, evaluate()).await {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(error)) if is_context_not_found_error(&error) => {
                // A missing-context rejection proves this attempt did not run,
                // so it is the sole error safe to retry. The sleep shares the
                // same deadline and cannot create another attempt afterward.
                if tokio::time::timeout_at(deadline, sleep(retry_delay))
                    .await
                    .is_err()
                {
                    return Err(Error::Cdp(CdpError::Timeout));
                }
            }
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err(Error::Cdp(CdpError::Timeout)),
        }
    }
}

fn is_context_not_found_error(error: &Error) -> bool {
    is_protocol_context_error(error, "Cannot find context with specified id")
}

fn is_context_destroyed_error(error: &Error) -> bool {
    is_protocol_context_error(error, "Execution context was destroyed.")
}

/// CDP command responses are converted into `CdpError::Chrome` by
/// chromiumoxide. Match both the protocol code and exact message, including
/// errors annotated with an automation stage.
fn is_protocol_context_error(error: &Error, expected_message: &str) -> bool {
    match error {
        Error::Cdp(CdpError::Chrome(protocol)) => {
            protocol.code == -32000 && protocol.message == expected_message
        }
        Error::Stage { source, .. } => is_protocol_context_error(source, expected_message),
        _ => false,
    }
}

/// Read-only polling may tolerate either form of context turnover.
fn is_context_turnover_error(error: &Error) -> bool {
    is_context_not_found_error(error) || is_context_destroyed_error(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_mode_requires_both_common_back_and_single_slot_layout() {
        assert_eq!(select_back_mode(true, true), ImageMode::Same);
        assert_eq!(select_back_mode(false, true), ImageMode::Different);
        assert_eq!(select_back_mode(true, false), ImageMode::Different);
    }

    #[test]
    fn absent_initial_slot_one_selects_the_single_layout() {
        assert!(initial_layout_is_single(2, false));
        assert!(!initial_layout_is_single(2, true));
        assert!(!initial_layout_is_single(1, false));
    }

    async fn acquire_sequence(states: &[&str]) -> usize {
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};

        let states = Arc::new(Mutex::new(
            states
                .iter()
                .map(|state| (*state).to_owned())
                .collect::<VecDeque<_>>(),
        ));
        let opens = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        acquire_back_chooser_with(
            Instant::now() + Duration::from_secs(1),
            Duration::ZERO,
            {
                let states = Arc::clone(&states);
                move |_| {
                    let state = states
                        .lock()
                        .expect("state queue lock")
                        .pop_front()
                        .expect("acquisition must not read beyond supplied states");
                    async move { Ok(state) }
                }
            },
            {
                let opens = Arc::clone(&opens);
                move || {
                    opens.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    async { Ok(()) }
                }
            },
        )
        .await
        .expect("sequence reaches ready");
        opens.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[tokio::test]
    async fn ready_chooser_does_not_open() {
        assert_eq!(acquire_sequence(&["ready"]).await, 0);
    }

    #[tokio::test]
    async fn loading_chooser_does_not_reopen_before_ready() {
        assert_eq!(acquire_sequence(&["loading", "ready"]).await, 0);
    }

    #[tokio::test]
    async fn openable_chooser_opens_once_then_waits_through_loading() {
        assert_eq!(acquire_sequence(&["openable", "loading", "ready"]).await, 1);
    }

    #[tokio::test]
    async fn slot_wait_transitions_from_uninitialized_index_through_missing_to_ready() {
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};

        let states = Arc::new(Mutex::new(VecDeque::from([
            "uninitialized-index".to_owned(),
            "missing-slot".to_owned(),
            "ready".to_owned(),
        ])));
        wait_for_editor_state_with(
            "editor-slot/wait-ready",
            Instant::now() + Duration::from_secs(1),
            Duration::ZERO,
            {
                let states = Arc::clone(&states);
                move |_| {
                    let state = states
                        .lock()
                        .expect("state queue lock")
                        .pop_front()
                        .unwrap();
                    async move { Ok(state) }
                }
            },
        )
        .await
        .expect("slot becomes ready");
    }

    #[tokio::test]
    async fn slot_wait_timeout_reports_its_last_state() {
        let error = wait_for_editor_state_with(
            "editor-slot/wait-ready",
            Instant::now() + Duration::from_millis(10),
            Duration::from_secs(1),
            |_| async { Ok("missing-slot".to_owned()) },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "editor-slot/wait-ready: timed out waiting for ready state (state: missing-slot)"
        );
    }

    #[tokio::test]
    async fn slot_wait_final_evaluation_timeout_keeps_uninitialized_index_state() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let error = wait_for_editor_state_with(
            "editor-slot/wait-ready",
            Instant::now() + Duration::from_millis(10),
            Duration::ZERO,
            |deadline| {
                let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if attempt == 0 {
                        Ok("uninitialized-index".to_owned())
                    } else {
                        tokio::time::timeout_at(deadline, std::future::pending::<()>())
                            .await
                            .map(|_| "ready".to_owned())
                            .map_err(|_| Error::Cdp(CdpError::Timeout))
                    }
                }
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "editor-slot/wait-ready: timed out waiting for ready state (state: uninitialized-index)"
        );
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn slot_wait_context_rejections_until_deadline_keep_uninitialized_index_state() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let error = wait_for_editor_state_with(
            "editor-slot/wait-ready",
            Instant::now() + Duration::from_millis(10),
            Duration::from_millis(1),
            |_| {
                let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if attempt == 0 {
                        Ok("uninitialized-index".to_owned())
                    } else {
                        Err(missing_context())
                    }
                }
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "editor-slot/wait-ready: timed out waiting for ready state (state: uninitialized-index)"
        );
        assert!(attempts.load(std::sync::atomic::Ordering::SeqCst) > 1);
    }

    #[tokio::test]
    async fn editor_state_wait_recovers_from_delayed_ready_state() {
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};

        let states = Arc::new(Mutex::new(VecDeque::from([
            "layer-loading".to_owned(),
            "ready".to_owned(),
        ])));
        wait_for_editor_state_with(
            "front-text/wait-ready",
            Instant::now() + Duration::from_secs(1),
            Duration::ZERO,
            {
                let states = Arc::clone(&states);
                move |_| {
                    let state = states
                        .lock()
                        .expect("state queue lock")
                        .pop_front()
                        .unwrap();
                    async move { Ok(state) }
                }
            },
        )
        .await
        .expect("the delayed ready state is accepted");
    }

    #[tokio::test]
    async fn editor_state_wait_recovers_from_context_turnover_read_probes() {
        let probes = std::sync::atomic::AtomicUsize::new(0);
        wait_for_editor_state_with(
            "front-text/wait-ready",
            Instant::now() + Duration::from_secs(1),
            Duration::ZERO,
            |_| {
                let probe = probes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    match probe {
                        0 => Err(missing_context()),
                        1 => Err(destroyed_context()),
                        _ => Ok("ready".to_owned()),
                    }
                }
            },
        )
        .await
        .expect("context-turnover read-only probes can be retried");
        assert_eq!(probes.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn editor_state_wait_propagates_permanent_probe_errors() {
        let error = wait_for_editor_state_with(
            "front-text/wait-ready",
            Instant::now() + Duration::from_secs(1),
            Duration::ZERO,
            |_| async { Err(Error::Site("permanent probe failure".into())) },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "front-text/wait-ready: permanent probe failure"
        );
    }

    #[tokio::test]
    async fn editor_state_wait_propagates_evaluation_timeout_before_deadline() {
        let error = wait_for_editor_state_with(
            "front-text/wait-ready",
            Instant::now() + Duration::from_secs(1),
            Duration::ZERO,
            |_| async { Err(Error::Cdp(CdpError::Timeout)) },
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "front-text/wait-ready: Request timed out.");
    }

    #[tokio::test]
    async fn editor_state_timeout_after_sleep_keeps_last_state_without_another_probe() {
        let probes = std::sync::atomic::AtomicUsize::new(0);
        let error = wait_for_editor_state_with(
            "front-text/wait-ready",
            Instant::now() + Duration::from_millis(10),
            Duration::from_secs(1),
            |_| {
                probes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Ok("layer-loading".to_owned()) }
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "front-text/wait-ready: timed out waiting for ready state (state: layer-loading)"
        );
        assert_eq!(probes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn absent_backs_destination_times_out_with_last_state_without_replaying_mutation() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let advances = AtomicUsize::new(0);
        let destination_probes = AtomicUsize::new(0);
        let error = advance_front_text_to_backs_with(
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_millis(20),
            Duration::from_millis(1),
            |_| async { Ok("ready".to_owned()) },
            || {
                advances.fetch_add(1, Ordering::SeqCst);
                async { Ok(()) }
            },
            |_| {
                destination_probes.fetch_add(1, Ordering::SeqCst);
                async { Ok("wrong-stage".to_owned()) }
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "backs/wait-backs: timed out waiting for ready state (state: wrong-stage)"
        );
        assert_eq!(advances.load(Ordering::SeqCst), 1);
        assert!(destination_probes.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn destroyed_front_text_advance_is_proven_by_backs_without_replay() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let advances = Arc::new(AtomicUsize::new(0));
        advance_front_text_to_backs_with(
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(1),
            Duration::ZERO,
            |_| async { Ok("ready".to_owned()) },
            {
                let advances = Arc::clone(&advances);
                move || {
                    advances.fetch_add(1, Ordering::SeqCst);
                    async { Err(destroyed_context()) }
                }
            },
            |_| async { Ok("ready".to_owned()) },
        )
        .await
        .expect("the destination proves the single mutation succeeded");
        assert_eq!(advances.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn front_text_advance_failure_is_surfaced_without_destination_probe() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let advances = Arc::new(AtomicUsize::new(0));
        let backs_reads = Arc::new(AtomicUsize::new(0));
        let error = advance_front_text_to_backs_with(
            Instant::now() + Duration::from_secs(1),
            Instant::now() + Duration::from_secs(1),
            Duration::ZERO,
            |_| async { Ok("ready".to_owned()) },
            {
                let advances = Arc::clone(&advances);
                move || {
                    advances.fetch_add(1, Ordering::SeqCst);
                    async { Err(Error::Site("advance failed".into())) }
                }
            },
            {
                let backs_reads = Arc::clone(&backs_reads);
                move |_| {
                    backs_reads.fetch_add(1, Ordering::SeqCst);
                    async { Ok("ready".to_owned()) }
                }
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "front-text/advance: advance failed");
        assert_eq!(advances.load(Ordering::SeqCst), 1);
        assert_eq!(backs_reads.load(Ordering::SeqCst), 0);
    }

    fn protocol_error(code: i64, message: &str) -> Error {
        Error::Cdp(CdpError::Chrome(chromiumoxide::types::Error {
            code,
            message: message.into(),
        }))
    }

    fn missing_context() -> Error {
        protocol_error(-32000, "Cannot find context with specified id")
    }

    fn destroyed_context() -> Error {
        protocol_error(-32000, "Execution context was destroyed.")
    }

    #[tokio::test]
    async fn ordinary_first_evaluation_keeps_the_normal_cdp_timeout() {
        let value = recover_missing_context(Duration::from_millis(1), Duration::ZERO, || async {
            sleep(Duration::from_millis(10)).await;
            Ok::<_, Error>(42)
        })
        .await
        .expect("the recovery duration must not bound the first evaluation");
        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn missing_context_starts_a_bounded_recovery() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let error = recover_missing_context(Duration::from_millis(5), Duration::ZERO, || {
            let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    Err(missing_context())
                } else {
                    std::future::pending::<Result<()>>().await
                }
            }
        })
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Cdp(CdpError::Timeout)));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retries_actual_missing_context_rejection_until_evaluation_succeeds() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let value = retry_missing_context(
            Instant::now() + Duration::from_millis(50),
            Duration::ZERO,
            || {
                let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if attempt == 0 {
                        Err(missing_context())
                    } else {
                        Ok(42)
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(value, 42);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn pending_evaluation_times_out_without_replay() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let error = retry_missing_context(
            Instant::now() + Duration::from_millis(5),
            Duration::ZERO,
            || {
                attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::pending::<Result<()>>()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Cdp(CdpError::Timeout)));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn elapsed_deadline_does_not_start_an_evaluation() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let error = retry_missing_context(Instant::now(), Duration::ZERO, || {
            attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async { Ok::<(), Error>(()) }
        })
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Cdp(CdpError::Timeout)));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn nested_retry_budget_expires_during_sleep_without_another_attempt() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let error = retry_missing_context(
            Instant::now() + Duration::from_millis(5),
            Duration::from_secs(1),
            || {
                attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Err::<(), _>(missing_context()) }
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Cdp(CdpError::Timeout)));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn exact_missing_context_rejection_is_retried() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let value = retry_missing_context(
            Instant::now() + Duration::from_millis(50),
            Duration::ZERO,
            || {
                let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if attempt == 0 {
                        Err(missing_context())
                    } else {
                        Ok(7)
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(value, 7);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn timeout_destroyed_context_and_permanent_errors_are_never_replayed() {
        for error in [
            Error::Cdp(CdpError::Timeout),
            destroyed_context(),
            Error::Site("permanent failure".into()),
        ] {
            let attempts = std::sync::atomic::AtomicUsize::new(0);
            let returned =
                recover_missing_context(Duration::from_millis(50), Duration::ZERO, || {
                    attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let error = match &error {
                        Error::Cdp(CdpError::Timeout) => Error::Cdp(CdpError::Timeout),
                        Error::Cdp(_) => destroyed_context(),
                        Error::Site(message) => Error::Site(message.clone()),
                        _ => unreachable!(),
                    };
                    async move { Err::<(), _>(error) }
                })
                .await
                .unwrap_err();
            assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert!(!is_context_not_found_error(&returned));
        }
    }

    #[tokio::test]
    async fn persistent_spinner_reloads_after_a_poll_sleep_crosses_the_deadline() {
        let probes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reloads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let result = wait_for_idle_with(
            Duration::from_millis(20),
            Duration::from_millis(1),
            {
                let probes = std::sync::Arc::clone(&probes);
                move |_| {
                    let probes = std::sync::Arc::clone(&probes);
                    async move {
                        probes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(true)
                    }
                }
            },
            {
                let reloads = std::sync::Arc::clone(&reloads);
                move || {
                    let reloads = std::sync::Arc::clone(&reloads);
                    async move {
                        reloads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    }
                }
            },
        )
        .await;

        assert_eq!(result.unwrap(), true);
        assert!(probes.load(std::sync::atomic::Ordering::SeqCst) > 0);
        assert_eq!(reloads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn stage_preserves_timeout_formatting() {
        assert_eq!(
            Error::stage("auth/check", Error::Cdp(CdpError::Timeout)).to_string(),
            "auth/check: Request timed out."
        );
    }

    #[test]
    fn protocol_context_classifiers_match_actual_chrome_errors_and_stages() {
        let missing = missing_context();
        let destroyed = destroyed_context();
        assert!(is_context_not_found_error(&missing));
        assert!(is_context_destroyed_error(&destroyed));
        assert!(is_context_turnover_error(&Error::stage("poll", missing)));
        assert!(is_context_turnover_error(&Error::stage("poll", destroyed)));
        assert!(!is_context_not_found_error(&protocol_error(
            -32001,
            "Cannot find context with specified id"
        )));
        assert!(!is_context_destroyed_error(&protocol_error(
            -32000,
            "Execution context was destroyed later"
        )));
        assert!(!is_context_turnover_error(&Error::Cdp(CdpError::Timeout)));
    }
}

/// Auto-accept any `alert`/`confirm`/`beforeunload` dialog the editor raises;
/// an unhandled dialog blocks the page's JS and hangs `evaluate`.
async fn auto_accept_dialogs(page: &Page) -> Result<()> {
    let mut dialogs = page
        .event_listener::<EventJavascriptDialogOpening>()
        .await?;
    // The accept loop runs concurrently with the driver's own use of the page, so
    // it needs its own handle; a chromiumoxide `Page` clone is a cheap refcount.
    let page = page.clone();
    tokio::spawn(async move {
        while dialogs.next().await.is_some() {
            if let Ok(params) = HandleJavaScriptDialogParams::builder().accept(true).build() {
                let _ = page.execute(params).await;
            }
        }
    });
    Ok(())
}
