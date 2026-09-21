//! The session actor. A single task owns the parsed order, the downloaded-image
//! paths, and the browser driver, so none of that state is shared and no locks
//! are needed. Tauri commands talk to it over a channel and get replies back via
//! oneshot senders.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use autofill_automation::{Driver, Error as AutomationError, ImageMode};
use autofill_core::{download, DownloadJob, DownloadOutcome, Face, Order, PostProcess, Source};
use futures::stream::{self, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, oneshot};

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
    Download {
        reply: Reply<()>,
    },
    Fill {
        reply: Reply<()>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

#[derive(Serialize, Clone)]
pub struct OrderSummary {
    pub name: String,
    pub quantity: u32,
    pub stock: String,
    pub foil: bool,
    pub cards: Vec<CardView>,
}

#[derive(Serialize, Clone)]
pub struct CardView {
    pub face: &'static str,
    pub drive_id: String,
    pub name: String,
    pub slots: Vec<u32>,
    /// True for the order's common cardback, so the UI can set it apart.
    pub cardback: bool,
}

// Event payloads. `Clone` is required by Tauri's `Emitter::emit`; all are tiny.

#[derive(Serialize, Clone)]
struct DownloadEvent {
    drive_id: String,
    ok: bool,
    error: Option<String>,
}

#[derive(Serialize, Clone)]
struct CardStatusEvent {
    drive_id: String,
    status: &'static str,
}

#[derive(Serialize, Clone)]
struct ThumbReadyEvent {
    drive_id: String,
    path: String,
}

#[derive(Serialize, Clone)]
struct PhaseEvent {
    phase: &'static str,
}

/// Run the actor until every command sender has been dropped.
pub async fn run(mut inbox: mpsc::Receiver<Command>, app: AppHandle) {
    let mut session = Session::new(app);
    while let Some(command) = inbox.recv().await {
        match command {
            Command::LoadOrder { path, reply } => {
                let _ = reply.send(session.load_order(path).await);
            }
            Command::LoadThumbnails { reply } => {
                session.spawn_thumbnail_fetch();
                let _ = reply.send(Ok(()));
            }
            Command::Download { reply } => {
                let _ = reply.send(session.download().await);
            }
            Command::Fill { reply } => {
                let _ = reply.send(session.fill().await);
            }
            Command::Shutdown { reply } => {
                session.shutdown().await;
                let _ = reply.send(());
                break;
            }
        }
    }
}

struct Session {
    app: AppHandle,
    order: Option<Order>,
    cards_dir: PathBuf,
    /// Drive id -> path of a successfully fetched image, ready to upload.
    downloaded: HashMap<String, PathBuf>,
    driver: Option<Driver>,
    post: Option<PostProcess>,
}

impl Session {
    fn new(app: AppHandle) -> Self {
        Session {
            app,
            order: None,
            cards_dir: PathBuf::new(),
            downloaded: HashMap::new(),
            driver: None,
            post: Some(PostProcess::default()),
        }
    }

    async fn load_order(&mut self, path: PathBuf) -> Result<OrderSummary, String> {
        let order = Order::from_file(&path).await.map_err(|e| e.to_string())?;

        // Images land in a `cards/` folder beside the order file. Authorize that
        // exact folder for the asset protocol so thumbnails load regardless of
        // where the order file lives.
        self.cards_dir = path.parent().unwrap_or(&path).join("cards");
        // Recursive so the `thumbs/` subfolder is covered too.
        let _ = self
            .app
            .asset_protocol_scope()
            .allow_directory(&self.cards_dir, true);
        self.downloaded.clear();

        let cardback_id = order.cardback_id.as_deref();
        let cards = order
            .images()
            .map(|(face, card)| CardView {
                face: face_name(face),
                drive_id: card.drive_id.clone(),
                name: card.name.as_deref().unwrap_or(&card.drive_id).to_owned(),
                slots: card.slots.iter().copied().collect(),
                cardback: face == Face::Back && cardback_id == Some(card.drive_id.as_str()),
            })
            .collect();
        let summary = OrderSummary {
            name: order.name.clone(),
            quantity: order.details.quantity,
            stock: order.details.stock.label().to_owned(),
            foil: order.details.foil,
            cards,
        };

        self.order = Some(order);
        Ok(summary)
    }

    async fn download(&mut self) -> Result<(), String> {
        let order = self.order.as_ref().ok_or("no order loaded")?;
        let client = download::build_client();

        // Own each job's id so it can move into a download task while the order
        // stays put for the fill phase.
        let jobs: Vec<DownloadJob> = order
            .images()
            .map(|(_, card)| DownloadJob {
                drive_id: card.drive_id.clone(),
                source: card.source,
                dest: self.cards_dir.join(card.file_name()),
            })
            .collect();

        let post = self.post;
        let client_ref = &client;
        let mut outcomes = stream::iter(jobs)
            .map(|job| async move { download::download_one(job, client_ref, post).await })
            .buffer_unordered(DOWNLOAD_CONCURRENCY);

        while let Some(DownloadOutcome { drive_id, result }) = outcomes.next().await {
            let event = match result {
                Ok(full) => {
                    // `drive_id` has two owners here — the event and the map — so
                    // one clone is unavoidable; everything else moves.
                    let event = DownloadEvent {
                        drive_id: drive_id.clone(),
                        ok: true,
                        error: None,
                    };
                    self.downloaded.insert(drive_id, full);
                    event
                }
                Err(error) => DownloadEvent {
                    drive_id,
                    ok: false,
                    error: Some(error),
                },
            };
            let _ = self.app.emit("download-progress", event);
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

        if self.driver.is_none() {
            let chrome = std::env::var("CHROME").ok();
            let driver = Driver::launch(chrome.as_deref())
                .await
                .map_err(|e| e.to_string())?;
            self.driver = Some(driver);
        }

        // Immutable borrows for the rest of the fill; nothing here mutates `self`.
        let driver = self.driver.as_ref().expect("driver launched above");
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
            let Some(path) = self.downloaded.get(&card.drive_id) else {
                self.emit_card(&card.drive_id, "skipped");
                continue;
            };
            self.emit_card(&card.drive_id, "uploading");
            let slots = effective_slots(face, back_mode, &card.slots);
            driver
                .upload_and_insert(path, &slots)
                .await
                .map_err(|e| e.to_string())?;
            self.emit_card(&card.drive_id, "inserted");
        }
        Ok(())
    }

    async fn shutdown(&mut self) {
        if let Some(driver) = self.driver.take() {
            driver.close().await;
        }
    }

    /// Kick off backend thumbnail fetching in a detached task so it never blocks
    /// the actor (the user can start a fill while previews are still loading).
    fn spawn_thumbnail_fetch(&self) {
        let Some(order) = self.order.as_ref() else {
            return;
        };
        let thumbs_dir = self.cards_dir.join("thumbs");

        let mut seen = HashSet::new();
        let jobs: Vec<ThumbJob> = order
            .images()
            .filter(|(_, card)| card.source == Source::GoogleDrive)
            .filter(|(_, card)| seen.insert(card.drive_id.as_str()))
            .map(|(_, card)| ThumbJob {
                url: format!(
                    "https://drive.google.com/thumbnail?id={}&sz=w256",
                    card.drive_id
                ),
                path: thumbs_dir.join(format!("{}.jpg", card.drive_id)),
                drive_id: card.drive_id.clone(),
            })
            .collect();

        let app = self.app.clone();
        tauri::async_runtime::spawn(fetch_thumbnails(jobs, app));
    }

    fn emit_phase(&self, phase: &'static str) {
        let _ = self.app.emit("phase", PhaseEvent { phase });
    }

    fn emit_card(&self, drive_id: &str, status: &'static str) {
        let _ = self.app.emit(
            "card-status",
            CardStatusEvent {
                drive_id: drive_id.to_owned(),
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
const THUMBNAIL_EDGE: u32 = 160;

struct ThumbJob {
    drive_id: String,
    url: String,
    path: PathBuf,
}

/// Fetch each card's Drive thumbnail in the backend (which has working TLS),
/// re-encode it small, cache it to disk, and tell the UI its local path. The UI
/// then loads it via the asset protocol — no webview networking involved.
async fn fetch_thumbnails(jobs: Vec<ThumbJob>, app: AppHandle) {
    let client = download::build_client();
    let client_ref = &client;

    let mut results = stream::iter(jobs)
        .map(|job| async move {
            let ok = download::fetch_thumbnail(client_ref, &job.url, &job.path, THUMBNAIL_EDGE)
                .await
                .is_ok();
            (job.drive_id, if ok { Some(job.path) } else { None })
        })
        .buffer_unordered(THUMBNAIL_CONCURRENCY);

    while let Some((drive_id, path)) = results.next().await {
        if let Some(path) = path {
            let _ = app.emit(
                "thumb-ready",
                ThumbReadyEvent {
                    drive_id,
                    path: path.to_string_lossy().into_owned(),
                },
            );
        }
    }
}
