//! The session actor. A single task owns the parsed order, the downloaded-image
//! paths, and the browser driver, so none of that state is shared and no locks
//! are needed. Tauri commands talk to it over a channel and get replies back via
//! oneshot senders.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use autofill_automation::{Driver, Error as AutomationError, ImageMode};
use autofill_core::decklist::{ArtworkSelection, DecklistPreview};
use autofill_core::new_order::NewOrderOptions;
use autofill_core::{download, DownloadJob, DownloadOutcome, Face, Order, PostProcess, Source};
use futures::stream::{self, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, oneshot};

use crate::files::write_atomic;
use crate::local_artwork::LocalArtwork;
use crate::saved_decks::SavedDecks;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

/// How many images to fetch and process at once.
const DOWNLOAD_CONCURRENCY: usize = 8;

pub type Reply<T> = oneshot::Sender<Result<T, String>>;

/// Messages the actor accepts. Each carries a `Reply` the command awaits.
pub enum Command {
    LoadOrder {
        path: PathBuf,
        reply: Reply<OrderSummary>,
    },
    LoadThumbnails {
        reply: Reply<()>,
    },
    CreateOrder {
        options: NewOrderOptions,
        text: String,
        selections: Vec<ArtworkSelection>,
        reply: Reply<OrderSummary>,
    },
    GetDecklist {
        reply: Reply<String>,
    },
    PreviewDecklist {
        text: String,
        selections: Vec<ArtworkSelection>,
        reply: Reply<DecklistPreview>,
    },
    EditDecklist {
        text: String,
        selections: Vec<ArtworkSelection>,
        options: NewOrderOptions,
        reply: Reply<OrderSummary>,
    },
    ChangeArtwork {
        drive_id: String,
        selection: ArtworkSelection,
        reply: Reply<OrderSummary>,
    },
    ApproveLocalImage {
        drive_id: String,
        reply: Reply<OrderSummary>,
    },
    ReplaceLocalImage {
        drive_id: String,
        reply: Reply<OrderSummary>,
    },
    SaveOrder {
        path: Option<PathBuf>,
        reply: Reply<OrderSummary>,
    },
    Download {
        reply: Reply<()>,
    },
    Fill {
        reply: Reply<()>,
    },
}

#[derive(Serialize, Clone)]
pub struct OrderSummary {
    pub session_id: u64,
    pub name: String,
    pub quantity: u32,
    pub stock: String,
    pub foil: bool,
    pub cards: Vec<CardView>,
    pub path: String,
    pub dirty: bool,
}

#[derive(Serialize, Clone)]
pub struct CardView {
    pub face: &'static str,
    pub drive_id: String,
    pub name: String,
    pub slots: Vec<u32>,
    pub query: Option<String>,
    /// True for the order's common cardback, so the UI can set it apart.
    pub cardback: bool,
    pub source: Source,
    pub local_approved: bool,
}

// Event payloads. `Clone` is required by Tauri's `Emitter::emit`; all are tiny.

#[derive(Serialize, Clone)]
struct DownloadEvent {
    drive_id: String,
    source: Source,
    ok: bool,
    error: Option<String>,
}

#[derive(Serialize, Clone)]
struct CardStatusEvent {
    drive_id: String,
    source: Source,
    face: &'static str,
    status: &'static str,
}

#[derive(Serialize, Clone)]
struct ThumbReadyEvent {
    session_id: u64,
    drive_id: String,
    source: Source,
    path: Option<String>,
    error: Option<String>,
}

#[derive(Serialize, Clone)]
struct PhaseEvent {
    phase: &'static str,
}

/// Shutdown is separate from the inbox and can interrupt long network work.
pub async fn run(
    mut inbox: mpsc::Receiver<Command>,
    app: AppHandle,
    shutdown: crate::shutdown::Shutdown,
) {
    let mut session = Session::new(app);
    loop {
        let command = tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            command = inbox.recv() => match command { Some(command) => command, None => break },
        };
        // Do not cancel a save halfway through its atomic replacement.
        if matches!(&command, Command::SaveOrder { .. }) {
            session.handle(command).await;
        } else {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => break,
                _ = session.handle(command) => {},
            }
        }
    }
    inbox.close();
    session.shutdown().await;
}

impl Session {
    async fn handle(&mut self, command: Command) {
        match command {
            Command::LoadOrder { path, reply } => {
                let _ = reply.send(self.load_order(path).await);
            }
            Command::LoadThumbnails { reply } => {
                self.spawn_thumbnail_fetch();
                let _ = reply.send(Ok(()));
            }
            Command::CreateOrder {
                options,
                text,
                selections,
                reply,
            } => {
                let _ = reply.send(self.create_order(&options, &text, &selections));
            }
            Command::GetDecklist { reply } => {
                let _ = reply.send(
                    self.order
                        .as_ref()
                        .ok_or_else(|| "no order loaded".to_owned())
                        .and_then(|order| order.to_decklist().map_err(|e| e.to_string())),
                );
            }
            Command::PreviewDecklist {
                text,
                selections,
                reply,
            } => {
                let _ = reply.send(
                    self.order
                        .as_ref()
                        .ok_or_else(|| "no order loaded".to_owned())
                        .and_then(|order| {
                            order
                                .preview_decklist(&text, &selections)
                                .map_err(|e| e.to_string())
                        }),
                );
            }
            Command::EditDecklist {
                text,
                selections,
                options,
                reply,
            } => {
                let _ = reply.send(self.edit_decklist(&text, &selections, &options));
            }
            Command::ChangeArtwork {
                drive_id,
                selection,
                reply,
            } => {
                let _ = reply.send(self.change_artwork(&drive_id, &selection));
            }
            Command::ApproveLocalImage { drive_id, reply } => {
                let _ = reply.send(self.review_local_image(&drive_id, false).await);
            }
            Command::ReplaceLocalImage { drive_id, reply } => {
                let _ = reply.send(self.review_local_image(&drive_id, true).await);
            }
            Command::SaveOrder { path, reply } => {
                let _ = reply.send(self.save_order(path).await);
            }
            Command::Download { reply } => {
                let _ = reply.send(self.download().await);
            }
            Command::Fill { reply } => {
                let _ = reply.send(self.fill().await);
            }
        }
    }
}

struct Session {
    session_id: u64,
    app: AppHandle,
    order: Option<Order>,
    cards_dir: PathBuf,
    /// Drive id -> path of a successfully fetched image, ready to upload.
    downloaded: HashMap<(Source, String), PathBuf>,
    local_artwork: Option<LocalArtwork>,
    driver: Option<Driver>,
    post: Option<PostProcess>,
    order_path: PathBuf,
    dirty: bool,
    background: Vec<tauri::async_runtime::JoinHandle<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        for task in &self.background {
            task.abort();
        }
    }
}

impl Session {
    fn new(app: AppHandle) -> Self {
        Session {
            session_id: 0,
            app,
            order: None,
            cards_dir: PathBuf::new(),
            downloaded: HashMap::new(),
            local_artwork: None,
            driver: None,
            post: Some(PostProcess::default()),
            order_path: PathBuf::new(),
            dirty: false,
            background: Vec::new(),
        }
    }

    async fn load_order(&mut self, path: PathBuf) -> Result<OrderSummary, String> {
        let order = Order::from_file(&path).await.map_err(|e| e.to_string())?;

        self.prepare_cache(&path)?;
        self.order_path = path;
        self.dirty = false;
        self.order = Some(order);
        self.remember_order().await;
        self.summary()
    }

    async fn remember_order(&self) {
        if let Err(error) = self
            .app
            .state::<SavedDecks>()
            .remember(&self.order_path)
            .await
        {
            tracing::warn!("could not remember saved deck: {error}");
            let _ = self.app.emit(
                "library-warning",
                format!("Your order is available, but could not be added to Saved decks: {error}"),
            );
        }
    }

    fn summary(&self) -> Result<OrderSummary, String> {
        let order = self.order.as_ref().ok_or("no order loaded")?;
        let cardback_id = order.cardback_id.as_deref();
        let cards = order
            .images()
            .map(|(face, card)| CardView {
                face: face_name(face),
                drive_id: card.drive_id.clone(),
                name: card.name.as_deref().unwrap_or(&card.drive_id).to_owned(),
                query: card.query.clone(),
                slots: card.slots.iter().copied().collect(),
                cardback: face == Face::Back && cardback_id == Some(card.drive_id.as_str()),
                source: card.source,
                local_approved: card.source == Source::LocalFile
                    && self
                        .local_artwork
                        .as_ref()
                        .and_then(|local| local.get(&card.drive_id))
                        .is_some(),
            })
            .collect();
        let summary = OrderSummary {
            session_id: self.session_id,
            name: order.name.clone(),
            quantity: order.details.quantity,
            stock: order.details.stock.label().to_owned(),
            foil: order.details.foil,
            cards,
            path: self.order_path.to_string_lossy().into_owned(),
            dirty: self.dirty,
        };

        Ok(summary)
    }

    fn create_order(
        &mut self,
        options: &NewOrderOptions,
        text: &str,
        selections: &[ArtworkSelection],
    ) -> Result<OrderSummary, String> {
        // Resolve and validate the complete draft before replacing a loaded order.
        let order = options
            .create(text, selections)
            .map_err(|e| e.to_string())?;
        self.prepare_cache(std::path::Path::new(""))?;
        self.order_path = PathBuf::new();
        self.downloaded.clear();
        self.dirty = true;
        self.order = Some(order);
        self.summary()
    }

    fn edit_decklist(
        &mut self,
        text: &str,
        selections: &[ArtworkSelection],
        options: &NewOrderOptions,
    ) -> Result<OrderSummary, String> {
        let edited = options
            .revise(
                self.order.as_ref().ok_or("no order loaded")?,
                text,
                selections,
            )
            .map_err(|e| e.to_string())?;
        self.dirty |= self.order.as_ref() != Some(&edited);
        self.order = Some(edited);
        self.downloaded.clear();
        self.summary()
    }

    fn change_artwork(
        &mut self,
        drive_id: &str,
        selection: &ArtworkSelection,
    ) -> Result<OrderSummary, String> {
        let edited = self
            .order
            .as_ref()
            .ok_or("no order loaded")?
            .change_artwork(drive_id, selection)
            .map_err(|e| e.to_string())?;
        if self.order.as_ref() != Some(&edited) {
            self.dirty = true;
            self.order = Some(edited);
            self.downloaded.clear();
        }
        self.summary()
    }

    async fn save_order(&mut self, path: Option<PathBuf>) -> Result<OrderSummary, String> {
        let path = path.unwrap_or_else(|| self.order_path.clone());
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_none_or(|s| !s.eq_ignore_ascii_case("xml"))
        {
            return Err("save the order with an .xml extension".into());
        }
        let xml = self
            .order
            .as_ref()
            .ok_or("no order loaded")?
            .to_xml()
            .map_err(|e| e.to_string())?;
        let dest = path.clone();
        tokio::task::spawn_blocking(move || write_atomic(&dest, &xml))
            .await
            .map_err(|e| e.to_string())??;
        if let Some(order) = self.order.as_mut() {
            order.name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Project")
                .into();
        }
        self.order_path = path;
        self.dirty = false;
        self.remember_order().await;
        self.summary()
    }

    fn prepare_cache(&mut self, order_path: &std::path::Path) -> Result<(), String> {
        let root = self
            .app
            .path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("images-v2");
        autofill_core::cache::private_directory(&root)?;
        let local = LocalArtwork::new(&root, order_path)?;
        self.cards_dir = root;
        self.local_artwork = Some(local);
        self.invalidate_previews();
        self.downloaded.clear();
        Ok(())
    }

    fn invalidate_previews(&mut self) {
        for task in self.background.drain(..) {
            task.abort();
        }
        // Events already queued by the webview must also be rejected, including
        // when a replacement refreshes a local snapshot at the same path.
        self.session_id = self.session_id.wrapping_add(1);
    }

    async fn review_local_image(
        &mut self,
        reference: &str,
        replace: bool,
    ) -> Result<OrderSummary, String> {
        let order = self.order.as_ref().ok_or("no order loaded")?;
        let local = self
            .local_artwork
            .as_mut()
            .ok_or("local image review is unavailable")?;
        let app = self.app.clone();
        let reviewed = local.review(order, reference, replace, |path| async move {
            if replace {
                let (send, receive) = oneshot::channel();
                app.dialog().file().set_title("Choose replacement local artwork")
                    .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                    .pick_file(move |value| { let _ = send.send(value); });
                receive.await.map_err(|e| e.to_string())?.map(|path| path.into_path().map_err(|e| e.to_string())).transpose()
            } else {
                let (send, receive) = oneshot::channel();
                app.dialog().message(format!("Allow this deck to read the image below and upload a copy to MakePlayingCards when you start autofill?\n\n{}\n\nOnly JPEG, PNG and WebP images are accepted. The original file will not be changed.", path.display()))
                    .title("Allow local image")
                    .buttons(MessageDialogButtons::OkCancelCustom("Allow image".into(), "Cancel".into()))
                    .show(move |value| { let _ = send.send(value); });
                Ok(receive.await.map_err(|e| e.to_string())?.then_some(path))
            }
        }).await?;
        let Some(reviewed) = reviewed else {
            return self.summary();
        };
        let new_reference = reviewed.reference;
        if replace && new_reference != reference {
            self.order = Some(crate::local_artwork::replace_reference(
                self.order.as_ref().expect("order checked above"),
                reference,
                &new_reference,
            )?);
            self.dirty = true;
        }
        self.invalidate_previews();
        self.downloaded.clear();
        self.summary()
    }

    async fn download(&mut self) -> Result<(), String> {
        let order = self.order.as_ref().ok_or("no order loaded")?;
        // The production gate validates the full order and every local grant
        // before creating a network job or starting Chrome.
        let approved = self
            .local_artwork
            .as_ref()
            .ok_or("image session is unavailable")?
            .approved_paths(order)?;
        for (source, drive_id) in approved.keys() {
            let _ = self.app.emit(
                "download-progress",
                DownloadEvent {
                    drive_id: drive_id.clone(),
                    source: *source,
                    ok: true,
                    error: None,
                },
            );
        }
        self.downloaded.extend(approved);
        let client = download::build_client();
        let mut seen = HashSet::new();
        let jobs: Vec<DownloadJob> = order
            .images()
            .filter(|(_, card)| {
                card.source == Source::GoogleDrive && seen.insert(card.drive_id.clone())
            })
            .map(|(_, card)| DownloadJob {
                drive_id: card.drive_id.clone(),
                source: card.source,
                dest: self.cards_dir.join(autofill_core::cache::image_key(
                    "drive",
                    &card.drive_id,
                    "original-800dpi-v2",
                )),
            })
            .collect();
        let post = self.post;
        let client_ref = &client;
        let mut outcomes = stream::iter(jobs)
            .map(|job| async move { download::download_one(job, client_ref, post).await })
            .buffer_unordered(DOWNLOAD_CONCURRENCY);
        let mut failures = 0;
        while let Some(DownloadOutcome { drive_id, result }) = outcomes.next().await {
            let error = match result {
                Ok(path) => {
                    self.downloaded
                        .insert((Source::GoogleDrive, drive_id.clone()), path);
                    None
                }
                Err(error) => {
                    failures += 1;
                    Some(error)
                }
            };
            let _ = self.app.emit(
                "download-progress",
                DownloadEvent {
                    drive_id,
                    source: Source::GoogleDrive,
                    ok: error.is_none(),
                    error,
                },
            );
        }
        if failures > 0 {
            return Err(format!("Could not prepare {failures} artwork image(s). Review the highlighted cards and retry."));
        }
        Ok(())
    }

    async fn fill(&mut self) -> Result<(), String> {
        if self.order.is_none() {
            return Err("no order loaded".into());
        }

        // Ensure every image is on disk before we start driving the browser. This
        // is idempotent — already-downloaded files are skipped quickly.
        self.emit_phase("downloading");
        self.download().await?;

        if self.driver.as_ref().is_some_and(Driver::is_closed) {
            if let Some(driver) = self.driver.take() {
                driver.close().await;
            }
        }

        if self.driver.is_none() {
            self.emit_phase("starting-browser");
            let chrome = std::env::var("CHROME").ok();
            let driver = Driver::launch(
                chrome.as_deref(),
                &self
                    .app
                    .path()
                    .app_data_dir()
                    .map_err(|e| e.to_string())?
                    .join("automation/browser"),
            )
            .await
            .map_err(|e| e.to_string())?;
            self.driver = Some(driver);
        }

        let driver = self.driver.as_ref().expect("driver launched above");
        let result = tokio::select! {
            biased;
            _ = driver.wait_until_closed() => Err("browser/closed: The automation browser was closed. Your deck is unchanged.".into()),
            result = self.fill_in_browser(driver) => result,
        };
        if self.driver.as_ref().is_some_and(Driver::is_closed) {
            if let Some(driver) = self.driver.take() {
                driver.close().await;
            }
        }
        result
    }

    async fn fill_in_browser(&self, driver: &Driver) -> Result<(), String> {
        let order = self.order.as_ref().expect("order checked above");

        if !driver
            .is_authenticated()
            .await
            .map_err(|error| AutomationError::stage("auth/check", error).to_string())?
        {
            self.emit_phase("awaiting-sign-in");
            driver.prompt_sign_in().await.map_err(|e| e.to_string())?;
            driver
                .wait_until_authenticated()
                .await
                .map_err(|error| AutomationError::stage("auth/wait", error).to_string())?;
        }

        self.emit_phase("configuring");
        driver
            .goto_start()
            .await
            .map_err(|e| format!("config/start: {e}"))?;
        driver
            .define_project(&order.details)
            .await
            .map_err(|e| format!("config/project: {e}"))?;
        driver
            .page_to_fronts(order.details.quantity)
            .await
            .map_err(|e| format!("config/fronts: {e}"))?;

        self.emit_phase("uploading-fronts");
        self.insert_face(driver, order, Face::Front, None).await?;

        let common_back_eligible = eligible_common_back(order);
        self.emit_phase("paging-to-backs");
        let back_mode = driver
            .page_to_backs(common_back_eligible, order.details.quantity)
            .await
            .map_err(|e| format!("backs: {e}"))?;
        self.emit_phase("uploading-backs");
        self.insert_face(driver, order, Face::Back, Some(back_mode))
            .await?;

        self.emit_phase("paging-to-review");
        driver
            .page_to_review()
            .await
            .map_err(|e| format!("review: {e}"))?;
        self.emit_phase("done");
        Ok(())
    }

    async fn insert_face(
        &self,
        driver: &Driver,
        order: &Order,
        face: Face,
        back_mode: Option<ImageMode>,
    ) -> Result<(), String> {
        for (_, card) in order.images().filter(|(f, _)| *f == face) {
            let Some(path) = self.downloaded.get(&(card.source, card.drive_id.clone())) else {
                return Err(
                    "An artwork image is unavailable. Prepare images again before uploading."
                        .into(),
                );
            };
            self.emit_card(&card.drive_id, card.source, face, "uploading");
            let slots = effective_slots(face, back_mode, &card.slots);
            driver
                .upload_and_insert(path, &slots)
                .await
                .map_err(|e| e.to_string())?;
            self.emit_card(&card.drive_id, card.source, face, "inserted");
        }
        Ok(())
    }

    async fn shutdown(&mut self) {
        for task in self.background.drain(..) {
            task.abort();
        }
        if let Some(driver) = self.driver.take() {
            driver.close().await;
        }
    }

    /// Kick off backend thumbnail fetching in a detached task so it never blocks
    /// the actor (the user can start a fill while previews are still loading).
    fn spawn_thumbnail_fetch(&mut self) {
        let Some(order) = self.order.as_ref() else {
            return;
        };
        // Keep larger previews separate from the older 160px cache. Otherwise
        // cached thumbnails would remain blurry in the expanded card dialog.
        let thumbs_dir = self.cards_dir.join("thumbs").join("640");

        let mut seen = HashSet::new();
        let mut jobs: Vec<ThumbJob> = order
            .images()
            .filter(|(_, card)| card.source == Source::GoogleDrive)
            .filter(|(_, card)| seen.insert(card.drive_id.as_str()))
            .map(|(_, card)| ThumbJob {
                source: Source::GoogleDrive,
                snapshot: None,
                url: format!(
                    "https://drive.google.com/thumbnail?id={}&sz=w640",
                    card.drive_id
                ),
                path: thumbs_dir.join(format!(
                    "{}.jpg",
                    autofill_core::cache::image_key("drive", &card.drive_id, "thumbnail-640-v2")
                )),
                drive_id: card.drive_id.clone(),
            })
            .collect();

        for (_, card) in order
            .images()
            .filter(|(_, card)| card.source == Source::LocalFile)
        {
            if let Some(path) = self
                .local_artwork
                .as_ref()
                .and_then(|local| local.get(&card.drive_id))
            {
                jobs.push(ThumbJob {
                    drive_id: card.drive_id.clone(),
                    source: Source::LocalFile,
                    snapshot: Some(path.clone()),
                    url: String::new(),
                    path: self
                        .local_artwork
                        .as_ref()
                        .expect("snapshot has a session")
                        .thumbnail_path(&card.drive_id)
                        .expect("snapshot has an immutable thumbnail path"),
                });
            }
        }
        let app = self.app.clone();
        self.background.retain(|task| !task.inner().is_finished());
        self.background
            .push(tauri::async_runtime::spawn(fetch_thumbnails(
                jobs,
                app,
                self.session_id,
            )));
    }

    fn emit_phase(&self, phase: &'static str) {
        let _ = self.app.emit("phase", PhaseEvent { phase });
    }

    fn emit_card(&self, drive_id: &str, source: Source, face: Face, status: &'static str) {
        let _ = self.app.emit(
            "card-status",
            CardStatusEvent {
                drive_id: drive_id.to_owned(),
                source,
                face: face_name(face),
                status,
            },
        );
    }
}

/// A common back can use the editor's Same mode only when it is the sole back
/// image and covers every requested card exactly once.
fn eligible_common_back(order: &Order) -> bool {
    common_back_eligible(
        order.cardback_id.as_deref(),
        &order.backs,
        order.details.quantity,
    )
}

fn common_back_eligible(
    cardback_id: Option<&str>,
    backs: &[autofill_core::Card],
    quantity: u32,
) -> bool {
    let (Some(cardback_id), [card]) = (cardback_id, backs) else {
        return false;
    };
    card.drive_id == cardback_id && card.slots == (0..quantity).collect()
}

/// Same mode has one physical design slot. Never silently collapse an order in
/// Different mode, where each declared slot remains required.
fn effective_slots(
    face: Face,
    back_mode: Option<ImageMode>,
    slots: &std::collections::BTreeSet<u32>,
) -> std::collections::BTreeSet<u32> {
    if face == Face::Back && back_mode == Some(ImageMode::Same) {
        [0].into_iter().collect()
    } else {
        slots.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autofill_core::{Card, Source};

    fn card(id: &str, slots: impl IntoIterator<Item = u32>) -> Card {
        Card {
            drive_id: id.into(),
            source: Source::GoogleDrive,
            slots: slots.into_iter().collect(),
            name: None,
            query: None,
        }
    }

    #[test]
    fn common_back_requires_one_matching_card_covering_every_slot() {
        assert!(common_back_eligible(Some("back"), &[card("back", 0..3)], 3));
        assert!(!common_back_eligible(
            Some("back"),
            &[card("back", [0, 2])],
            3
        ));
        assert!(!common_back_eligible(
            Some("back"),
            &[card("back", 0..3), card("other", [])],
            3
        ));
        assert!(!common_back_eligible(
            Some("other"),
            &[card("back", 0..3)],
            3
        ));
    }

    #[test]
    fn only_same_back_mode_collapses_to_slot_zero() {
        let slots = [1, 2].into_iter().collect();
        assert_eq!(
            effective_slots(Face::Back, Some(ImageMode::Same), &slots),
            [0].into_iter().collect()
        );
        assert_eq!(
            effective_slots(Face::Back, Some(ImageMode::Different), &slots),
            slots
        );
        assert_eq!(
            effective_slots(Face::Front, Some(ImageMode::Same), &slots),
            slots
        );
    }

    #[test]
    fn saving_replaces_complete_xml_and_cleans_up_failed_writes() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("mpc-save-test-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("order.xml");
        std::fs::write(&path, "original").unwrap();
        write_atomic(&path, "<order>replacement</order>").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "<order>replacement</order>"
        );
        let blocked = dir.join("directory.xml");
        std::fs::create_dir(&blocked).unwrap();
        assert!(write_atomic(&blocked, "invalid destination").is_err());
        assert!(blocked.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(blocked).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}

fn face_name(face: Face) -> &'static str {
    match face {
        Face::Front => "front",
        Face::Back => "back",
    }
}

/// How many thumbnails to fetch at once. They're small, so we can be generous.
const THUMBNAIL_CONCURRENCY: usize = 12;
/// Longest edge of a cached UI thumbnail.
const THUMBNAIL_EDGE: u32 = 640;

struct ThumbJob {
    drive_id: String,
    source: Source,
    snapshot: Option<PathBuf>,
    url: String,
    path: PathBuf,
}

/// Fetch each card's Drive thumbnail in the backend (which has working TLS),
/// re-encode it small, cache it to disk, and tell the UI its local path. The UI
/// then loads it via the asset protocol — no webview networking involved.
async fn fetch_thumbnails(jobs: Vec<ThumbJob>, app: AppHandle, session_id: u64) {
    let client = download::build_client();
    let client_ref = &client;

    let mut results = stream::iter(jobs)
        .map(|job| async move {
            let result = if let Some(snapshot) = &job.snapshot {
                autofill_core::local_files::snapshot_thumbnail(snapshot, &job.path, THUMBNAIL_EDGE)
                    .await
                    .map(|_| ())
            } else {
                download::fetch_thumbnail(client_ref, &job.url, &job.path, THUMBNAIL_EDGE).await
            };
            (
                job.drive_id,
                job.source,
                job.path,
                result.err().map(|error| error.to_string()),
            )
        })
        .buffer_unordered(THUMBNAIL_CONCURRENCY);

    while let Some((drive_id, source, path, mut error)) = results.next().await {
        if error.is_none() {
            error = app
                .asset_protocol_scope()
                .allow_file(&path)
                .err()
                .map(|e| e.to_string());
        }
        let _ = app.emit(
            "thumb-ready",
            ThumbReadyEvent {
                session_id,
                drive_id,
                source,
                path: error.is_none().then(|| path.to_string_lossy().into_owned()),
                error,
            },
        );
    }
}
