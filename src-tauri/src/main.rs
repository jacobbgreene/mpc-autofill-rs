#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! MPC Autofill desktop app. The GUI is a thin shell: commands forward requests
//! to the session actor (see [`session`]) and the actor streams progress back to
//! the frontend as Tauri events.

mod artwork;
mod artwork_thumbnail;
mod files;
mod local_artwork;
mod saved_decks;
mod session;
mod shutdown;

use std::path::PathBuf;
use std::sync::Mutex;

use autofill_core::decklist::{ArtworkSelection, DecklistPreview};
use autofill_core::new_order::NewOrderOptions;
use session::{Command, OrderSummary};
use tauri::{Manager, State};
use tokio::sync::{mpsc, oneshot};

/// The only state the frontend touches: a handle to the session actor's inbox.
struct AppState {
    inbox: mpsc::Sender<Command>,
    shutdown: shutdown::Shutdown,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AppState {
    /// Send a command to the actor and await its reply. Every command is a
    /// request/response round-trip over a fresh oneshot channel.
    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, String>>) -> Command,
    ) -> Result<T, String> {
        let (reply, response) = oneshot::channel();
        self.shutdown
            .run(async {
                self.inbox
                    .send(build(reply))
                    .await
                    .map_err(|_| "session is not running")?;
                response.await.map_err(|_| "session dropped the request")?
            })
            .await
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
    options: NewOrderOptions,
) -> Result<OrderSummary, String> {
    state
        .request(|reply| Command::EditDecklist {
            text,
            selections,
            options,
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
async fn approve_local_image(
    state: State<'_, AppState>,
    drive_id: String,
) -> Result<OrderSummary, String> {
    state
        .request(|reply| Command::ApproveLocalImage { drive_id, reply })
        .await
}

#[tauri::command]
async fn replace_local_image(
    state: State<'_, AppState>,
    drive_id: String,
) -> Result<OrderSummary, String> {
    state
        .request(|reply| Command::ReplaceLocalImage { drive_id, reply })
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
    tokio::task::spawn_blocking(move || {
        autofill_core::validation::read_text_file_bounded(&path, 1_000_000)
    })
    .await
    .map_err(|e| e.to_string())?
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
            let shutdown = shutdown::Shutdown::default();
            let task = tauri::async_runtime::handle().inner().spawn(session::run(
                receiver,
                handle,
                shutdown.clone(),
            ));
            app.manage(shutdown.clone());
            app.manage(AppState {
                inbox,
                shutdown,
                task: Mutex::new(Some(task)),
            });
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
            approve_local_image,
            replace_local_image,
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
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if let Some(state) = app.try_state::<AppState>() {
                    if state.shutdown.finished() {
                        return;
                    }
                    api.prevent_exit();
                    if !state.shutdown.begin() {
                        return;
                    }
                    let task = state
                        .task
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take();
                    let shutdown = state.shutdown.clone();
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(task) = task {
                            shutdown::finish_session(task).await;
                        }
                        shutdown.finish();
                        app.exit(0);
                    });
                }
            }
        });
}
