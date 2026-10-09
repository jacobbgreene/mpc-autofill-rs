#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! MPC Autofill desktop app. The GUI is a thin shell: commands forward requests
//! to the session actor (see [`session`]) and the actor streams progress back to
//! the frontend as Tauri events.

mod artwork;
mod artwork_thumbnail;
mod draft;
mod files;
mod saved_decks;
mod session;

use std::path::PathBuf;
use std::time::Duration;

use autofill_core::decklist::{ArtworkSelection, DecklistPreview};
use autofill_core::new_order::NewOrderOptions;
use draft::DeckDraft;
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
async fn change_artwork(
    state: State<'_, AppState>,
    drive_id: String,
    selection: ArtworkSelection,
) -> Result<OrderSummary, String> {
    state
        .request(|reply| Command::ChangeArtwork {
            drive_id,
            selection,
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
    draft: DeckDraft,
) -> Result<OrderSummary, String> {
    state
        .request(|reply| Command::CreateOrder { draft, reply })
        .await
}

/// Longest the exit path waits for the actor to close the browser. The actor
/// handles commands sequentially, so a mid-fill or mid-download Shutdown can
/// only be serviced after the current command returns; without a bound, a
/// stuck operation would make the window un-closable.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

async fn request_shutdown(
    inbox: &mpsc::Sender<Command>,
    grace: Duration,
) -> Result<(), &'static str> {
    tokio::time::timeout(grace, async {
        let (reply, response) = oneshot::channel();
        inbox
            .send(Command::Shutdown { reply })
            .await
            .map_err(|_| "session is no longer running")?;
        response.await.map_err(|_| "session dropped shutdown")
    })
    .await
    .map_err(|_| "session did not shut down before the exit deadline")?
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .manage(artwork_thumbnail::ArtworkThumbnails::default())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(saved_decks::SavedDecks::new(
                app.path().app_data_dir()?,
                app.path().document_dir().ok(),
            ));
            let (inbox, receiver) = mpsc::channel(16);
            // The actor needs an owned handle to emit events; this is the one
            // framework handle we hand off, moved (not shared) into its task.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(session::run(receiver, handle));
            app.manage(AppState { inbox });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_order,
            thumbnails,
            download,
            fill,
            get_decklist,
            preview_decklist,
            edit_decklist,
            change_artwork,
            save_order,
            read_decklist,
            preview_new_decklist,
            create_order,
            artwork::search_artwork,
            artwork::artwork_dpi,
            artwork_thumbnail::artwork_thumbnail,
            saved_decks::list_saved_decks,
            saved_decks::default_deck_path
        ])
        .build(tauri::generate_context!())
        .expect("failed to start MPC Autofill")
        .run(|app, event| {
            // On exit, ask the actor to close the browser so we don't orphan the
            // visible Chrome window. Block briefly until it has torn down.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app.try_state::<AppState>() {
                    // The same deadline covers a full actor queue and browser
                    // teardown, so an in-flight command cannot pin exit open.
                    if let Err(error) = tauri::async_runtime::block_on(request_shutdown(
                        &state.inbox,
                        SHUTDOWN_GRACE,
                    )) {
                        tracing::warn!("{error}; exiting anyway");
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_deadline_includes_waiting_for_space_in_a_full_queue() {
        let (inbox, _receiver) = mpsc::channel(1);
        let (reply, _response) = oneshot::channel();
        inbox.send(Command::Download { reply }).await.unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            request_shutdown(&inbox, Duration::from_millis(10)),
        )
        .await
        .expect("a full queue must not prevent the exit deadline");
        assert_eq!(
            result,
            Err("session did not shut down before the exit deadline")
        );
    }

    #[tokio::test]
    async fn shutdown_waits_for_browser_teardown_acknowledgement() {
        let (inbox, mut receiver) = mpsc::channel(1);
        let actor = tokio::spawn(async move {
            match receiver.recv().await.unwrap() {
                Command::Shutdown { reply } => reply.send(()).unwrap(),
                _ => panic!("expected shutdown"),
            }
        });
        request_shutdown(&inbox, Duration::from_secs(1))
            .await
            .unwrap();
        actor.await.unwrap();
    }
}
