#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! MPC Autofill desktop app. The GUI is a thin shell: commands forward requests
//! to the session actor (see [`session`]) and the actor streams progress back to
//! the frontend as Tauri events.

mod artwork;
mod session;

use std::path::PathBuf;
use std::time::Duration;

use autofill_core::decklist::{ArtworkSelection, DecklistPreview};
use autofill_core::new_order::NewOrderOptions;
use session::{Command, OrderSummary};
use tauri::{Manager, State};
use tokio::sync::{mpsc, oneshot};

/// The only state the frontend touches: a handle to the session actor's inbox.
struct AppState {
    inbox: mpsc::Sender<Command>,
}

impl AppState {
    /// Send a command to the actor and await its reply. Every command is a
    /// request/response round-trip over a fresh oneshot channel.
    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, String>>) -> Command,
    ) -> Result<T, String> {
        let (reply, response) = oneshot::channel();
        self.inbox
            .send(build(reply))
            .await
            .map_err(|_| "session is not running")?;
        response.await.map_err(|_| "session dropped the request")?
    }
}

#[tauri::command]
async fn load_order(state: State<'_, AppState>, path: String) -> Result<OrderSummary, String> {
    state
        .request(|reply| Command::LoadOrder {
            path: PathBuf::from(path),
            reply,
        })
        .await
}

#[tauri::command]
async fn thumbnails(state: State<'_, AppState>) -> Result<(), String> {
    state
        .request(|reply| Command::LoadThumbnails { reply })
        .await
}

#[tauri::command]
async fn download(state: State<'_, AppState>) -> Result<(), String> {
    state.request(|reply| Command::Download { reply }).await
}

#[tauri::command]
async fn fill(state: State<'_, AppState>) -> Result<(), String> {
    state.request(|reply| Command::Fill { reply }).await
}

#[tauri::command]
async fn get_decklist(state: State<'_, AppState>) -> Result<String, String> {
    state.request(|reply| Command::GetDecklist { reply }).await
}

#[tauri::command]
async fn preview_decklist(
    state: State<'_, AppState>,
    text: String,
    selections: Vec<ArtworkSelection>,
) -> Result<DecklistPreview, String> {
    state
        .request(|reply| Command::PreviewDecklist {
            text,
            selections,
            reply,
        })
        .await
}

#[tauri::command]
async fn edit_decklist(
    state: State<'_, AppState>,
    text: String,
    selections: Vec<ArtworkSelection>,
) -> Result<OrderSummary, String> {
    state
        .request(|reply| Command::EditDecklist {
            text,
            selections,
            reply,
        })
        .await
}

#[tauri::command]
async fn save_order(
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<OrderSummary, String> {
    state
        .request(|reply| Command::SaveOrder {
            path: path.map(PathBuf::from),
            reply,
        })
        .await
}

#[tauri::command]
async fn read_decklist(path: String) -> Result<String, String> {
    let path = PathBuf::from(path);
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_none_or(|s| !s.eq_ignore_ascii_case("txt"))
    {
        return Err("choose a .txt decklist".into());
    }
    if tokio::fs::metadata(&path)
        .await
        .map_err(|e| e.to_string())?
        .len()
        > 1_000_000
    {
        return Err("decklist is too large; use a text file under 1 MB".into());
    }
    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn preview_new_decklist(
    options: NewOrderOptions,
    text: String,
    selections: Vec<ArtworkSelection>,
) -> Result<DecklistPreview, String> {
    options
        .preview(&text, &selections)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_order(
    state: State<'_, AppState>,
    options: NewOrderOptions,
    text: String,
    selections: Vec<ArtworkSelection>,
) -> Result<OrderSummary, String> {
    state
        .request(|reply| Command::CreateOrder {
            options,
            text,
            selections,
            reply,
        })
        .await
}

/// Longest the exit path waits for the actor to close the browser. The actor
/// handles commands sequentially, so a mid-fill or mid-download Shutdown can
/// only be serviced after the current command returns; without a bound, a
/// stuck operation would make the window un-closable.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let (inbox, receiver) = mpsc::channel(16);
            // The actor needs an owned handle to emit events; this is the one
            // framework handle we hand off, moved (not shared) into its task.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(session::run(receiver, handle));
            app.manage(AppState { inbox });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![load_order, thumbnails, download, fill, get_decklist, preview_decklist, edit_decklist, save_order, read_decklist, preview_new_decklist, create_order, artwork::search_artwork])
        .build(tauri::generate_context!())
        .expect("failed to start MPC Autofill")
        .run(|app, event| {
            // On exit, ask the actor to close the browser so we don't orphan the
            // visible Chrome window. Block briefly until it has torn down.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app.try_state::<AppState>() {
                    let (reply, response) = oneshot::channel();
                    if state.inbox.blocking_send(Command::Shutdown { reply }).is_ok() {
                        // Wait briefly for a clean browser teardown, then exit
                        // regardless — a hung command must not pin the window
                        // open. In that pathological case the live Chrome is
                        // orphaned; the next launch reaps it (see Driver::launch).
                        let expired = tauri::async_runtime::block_on(async {
                            tokio::time::timeout(SHUTDOWN_GRACE, response).await.is_err()
                        });
                        if expired {
                            tracing::warn!(
                                "session did not shut down within {SHUTDOWN_GRACE:?}; exiting anyway"
                            );
                        }
                    }
                }
            }
        });
}
