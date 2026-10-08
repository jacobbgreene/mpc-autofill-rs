//! Saved orders live in a user-owned folder; this index only remembers links.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use autofill_core::{Order, Source};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Mutex;

use crate::draft;
use crate::files::{write_atomic, write_new};

pub struct SavedDecks {
    folder: PathBuf,
    documents: Option<PathBuf>,
    index: PathBuf,
    gate: Mutex<()>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct Index {
    paths: Vec<PathBuf>,
    imported_documents: bool,
}

#[derive(Serialize)]
pub struct SavedDeck {
    path: String,
    name: String,
    quantity: u32,
    stock: String,
    foil: bool,
    modified: u64,
    preview_id: Option<String>,
    draft: bool,
}

#[derive(Serialize)]
pub struct Listing {
    decks: Vec<SavedDeck>,
    folder: String,
    warning: Option<String>,
}

impl SavedDecks {
    pub fn new(data: PathBuf, documents: Option<PathBuf>) -> Self {
        let folder = documents
            .as_ref()
            .map(|dir| dir.join("MPC Autofill").join("Decks"))
            .unwrap_or_else(|| data.join("decks"));
        Self {
            folder,
            documents,
            index: data.join("saved-decks.json"),
            gate: Mutex::new(()),
        }
    }

    pub async fn remember(&self, path: &Path) -> Result<(), String> {
        let _guard = self.gate.lock().await;
        let index_path = self.index.clone();
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut index = read_index(&index_path)?;
            let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
            if !index.paths.contains(&path) {
                index.paths.push(path);
                write_index(&index_path, &index)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub async fn save_new_order(&self, order: &Order) -> Result<PathBuf, String> {
        let _guard = self.gate.lock().await;
        let folder = self.folder.clone();
        let stem = safe_file_stem(&order.name);
        let xml = order.to_xml().map_err(|e| e.to_string())?;
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            for number in 1..=10_000 {
                let file_name = if number == 1 {
                    format!("{stem}.xml")
                } else {
                    format!("{stem} ({number}).xml")
                };
                let path = folder.join(file_name);
                match write_new(&path, &xml) {
                    Ok(()) => return Ok(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
            Err("too many decks with this name".into())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn list(&self) -> Result<Listing, String> {
        let _guard = self.gate.lock().await;
        let folder = self.folder.clone();
        let documents = self.documents.clone();
        let index = self.index.clone();
        tokio::task::spawn_blocking(move || list_files(&folder, documents.as_deref(), &index))
            .await
            .map_err(|e| e.to_string())
    }

    async fn default_path(&self, file_name: &str) -> Result<String, String> {
        // The picker may choose another location; this is only its initial path.
        if Path::new(file_name).file_name().and_then(|s| s.to_str()) != Some(file_name)
            || file_name.contains(['/', '\\'])
            || !is_deck_file(Path::new(file_name))
        {
            return Err("choose an XML order or .mpcdeck filename without a directory".into());
        }
        tokio::fs::create_dir_all(&self.folder)
            .await
            .map_err(|e| format!("Could not create the deck folder: {e}"))?;
        let requested = Path::new(file_name);
        let stem = safe_file_stem(
            requested
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Deck"),
        );
        let extension = requested.extension().and_then(|s| s.to_str()).unwrap();
        Ok(self
            .folder
            .join(format!("{stem}.{extension}"))
            .to_string_lossy()
            .into_owned())
    }
}

fn safe_file_stem(name: &str) -> String {
    let mut stem = String::new();
    for character in name.trim().chars() {
        let safe = if character.is_control() || "<>:\"/\\|?*".contains(character) {
            '_'
        } else {
            character
        };
        if stem.len() + safe.len_utf8() > 180 {
            break;
        }
        stem.push(safe);
    }
    let stem = stem.trim_matches([' ', '.']);
    if stem.is_empty() {
        return "Deck".into();
    }
    // Windows reserves these device names even when followed by extensions.
    let base = stem.split('.').next().unwrap_or(stem).to_ascii_uppercase();
    let device = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            base.strip_prefix(prefix).is_some_and(|number| {
                matches!(
                    number,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
    if device {
        format!("_{stem}")
    } else {
        stem.into()
    }
}

#[tauri::command]
pub async fn list_saved_decks(state: State<'_, SavedDecks>) -> Result<Listing, String> {
    state.list().await
}

#[tauri::command]
pub async fn default_deck_path(
    state: State<'_, SavedDecks>,
    file_name: String,
) -> Result<String, String> {
    state.default_path(&file_name).await
}

fn is_xml(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("xml"))
}

fn is_deck_file(path: &Path) -> bool {
    is_xml(path) || draft::is_draft(path)
}

fn read_index(path: &Path) -> Result<Index, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Index::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn write_index(path: &Path, index: &Index) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_atomic(
        path,
        &serde_json::to_string_pretty(index).map_err(|e| e.to_string())?,
    )
}

fn deck_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    match std::fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(|entry| match entry {
                Ok(entry) if is_deck_file(&entry.path()) => Some(Ok(entry.path())),
                Ok(_) => None,
                Err(e) => Some(Err(e.to_string())),
            })
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

fn describe(path: &Path) -> Result<SavedDeck, String> {
    if !is_deck_file(path) {
        return Err("not an order or deck draft".into());
    }
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let metadata = path.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 8 * 1024 * 1024 {
        return Err("not an order file under 8 MB".into());
    }
    let (name, quantity, stock, foil, preview_id, is_draft) = if draft::is_draft(&path) {
        let saved = draft::read(&path)?;
        let (quantity, _) = saved.preview();
        (
            saved.name,
            quantity,
            saved.stock.label().to_owned(),
            saved.foil,
            None,
            true,
        )
    } else {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Deck");
        let xml = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let order = Order::from_xml(name, &xml).map_err(|e| e.to_string())?;
        let preview_id = order
            .fronts
            .iter()
            .find(|card| {
                card.source == Source::GoogleDrive
                    && !card.drive_id.is_empty()
                    && card
                        .drive_id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
            .map(|card| card.drive_id.clone());
        (
            order.name,
            order.details.quantity,
            order.details.stock.label().to_owned(),
            order.details.foil,
            preview_id,
            false,
        )
    };
    Ok(SavedDeck {
        path: path.to_string_lossy().into_owned(),
        name,
        quantity,
        stock,
        foil,
        modified: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|time| time.as_millis() as u64)
            .unwrap_or(0),
        preview_id,
        draft: is_draft,
    })
}

fn list_files(folder: &Path, documents: Option<&Path>, index_path: &Path) -> Listing {
    let mut warnings = Vec::new();
    let (mut index, writable) = match read_index(index_path) {
        Ok(index) => (index, true),
        Err(error) => {
            warnings.push(format!("Could not read the saved-deck index: {error}"));
            (Index::default(), false)
        }
    };
    // Pick up orders saved by older versions once. Do not move files or scan
    // unrelated subdirectories. Future opens and saves register their paths.
    if !index.imported_documents {
        let legacy = documents.map(deck_files).transpose();
        match legacy {
            Ok(paths) => {
                for path in paths.into_iter().flatten() {
                    if let Ok(deck) = describe(&path) {
                        let path = PathBuf::from(deck.path);
                        if !index.paths.contains(&path) {
                            index.paths.push(path);
                        }
                    }
                }
                index.imported_documents = true;
                if writable {
                    if let Err(error) = write_index(index_path, &index) {
                        warnings.push(format!("Could not remember existing decks: {error}"));
                    }
                }
            }
            Err(error) => warnings.push(format!("Could not check Documents for decks: {error}")),
        }
    }
    let mut candidates: Vec<_> = index
        .paths
        .iter()
        .map(|path| (path.clone(), true))
        .collect();
    match deck_files(folder) {
        Ok(paths) => candidates.extend(paths.into_iter().map(|path| (path, false))),
        Err(error) => warnings.push(format!("Could not read the deck folder: {error}")),
    }
    let mut seen = HashSet::new();
    let mut decks = Vec::new();
    let mut unavailable = 0;
    for (path, remembered) in candidates {
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !seen.insert(key) {
            continue;
        }
        match describe(&path) {
            Ok(deck) => decks.push(deck),
            Err(_) if remembered => unavailable += 1,
            Err(_) => {} // Ignore unrelated XML files in the folder.
        }
    }
    if unavailable > 0 {
        warnings.push(format!(
            "{unavailable} remembered deck(s) are unavailable. Use Open order to locate moved files."
        ));
    }
    decks.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.name.cmp(&b.name))
    });
    Listing {
        decks,
        folder: folder.to_string_lossy().into_owned(),
        warning: (!warnings.is_empty()).then(|| warnings.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<order><details><quantity>3</quantity><stock>(S30) Standard Smooth</stock><foil>false</foil></details><fronts><card><id>front-art</id><slots>0,1,2</slots><name>Island.png</name></card></fronts><cardback>back-art</cardback></order>";

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("mpc-library-{}-{stamp}", std::process::id()));
            std::fs::create_dir_all(path.join("Documents")).unwrap();
            Self(path)
        }
        fn library(&self) -> SavedDecks {
            SavedDecks::new(self.0.join("data"), Some(self.0.join("Documents")))
        }
        fn deck(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, XML).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn named_decks_save_without_replacing_an_existing_deck() {
        let fixture = Fixture::new();
        let library = fixture.library();
        let mut order = Order::from_xml("My/Deck", XML).unwrap();
        let first = library.save_new_order(&order).await.unwrap();
        let original = std::fs::read_to_string(&first).unwrap();
        order.details.foil = true;
        let second = library.save_new_order(&order).await.unwrap();
        assert_eq!(first.file_name().unwrap(), "My_Deck.xml");
        assert_eq!(second.file_name().unwrap(), "My_Deck (2).xml");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), original);
        assert_eq!(Order::from_file(&first).await.unwrap().name, "My/Deck");
        let listing = library.list().await.unwrap();
        assert_eq!(listing.decks.len(), 2);
        assert!(listing.decks.iter().all(|deck| deck.name == "My/Deck"));
    }

    #[test]
    fn named_deck_filenames_are_portable_and_fit_component_limits() {
        for name in ["CON", "aux.notes", "Lpt9", "COM¹"] {
            assert_eq!(safe_file_stem(name), format!("_{name}"));
        }
        assert_eq!(safe_file_stem("  .. "), "Deck");
        assert_eq!(safe_file_stem("My/Deck"), "My_Deck");
        assert_eq!(safe_file_stem("COM10"), "COM10");
        assert!(safe_file_stem(&"é".repeat(200)).len() <= 180);
    }

    #[tokio::test]
    async fn discovers_managed_and_legacy_decks_without_moving_or_recursing() {
        let fixture = Fixture::new();
        let library = fixture.library();
        let legacy = fixture.deck("Documents/Old deck.XML");
        let managed = fixture.deck("Documents/MPC Autofill/Decks/New deck.xml");
        fixture.deck("Documents/unrelated/nested.xml");
        std::fs::write(fixture.0.join("Documents/settings.xml"), "<settings/>").unwrap();
        let listing = library.list().await.unwrap();
        assert_eq!(listing.decks.len(), 2);
        assert!(listing.warning.is_none());
        assert!(legacy.is_file() && managed.is_file());
        let old = listing.decks.iter().find(|d| d.name == "Old deck").unwrap();
        assert_eq!(old.quantity, 3);
        assert_eq!(old.preview_id.as_deref(), Some("front-art"));
        // Windows canonical paths expand short names and use an extended prefix.
        assert_eq!(old.path, legacy.canonicalize().unwrap().to_string_lossy());
        // One-time discovery never expands to later unrelated Documents files.
        fixture.deck("Documents/not-imported-later.xml");
        assert_eq!(fixture.library().list().await.unwrap().decks.len(), 2);
    }

    #[tokio::test]
    async fn remembers_external_paths_across_restarts_and_refreshes_file_metadata() {
        let fixture = Fixture::new();
        let path = fixture.deck("elsewhere/Smaug.xml");
        let library = fixture.library();
        library.remember(&path).await.unwrap();
        library.remember(&path).await.unwrap();
        assert_eq!(read_index(&library.index).unwrap().paths.len(), 1);
        let replacement = XML
            .replace("<quantity>3", "<quantity>4")
            .replace("<slots>0,1,2", "<slots>0,1,2,3")
            .replace("<foil>false", "<foil>true");
        std::fs::write(&path, replacement).unwrap();
        let listing = fixture.library().list().await.unwrap();
        assert_eq!(listing.decks.len(), 1);
        assert_eq!(listing.decks[0].quantity, 4);
        assert!(listing.decks[0].foil);
        assert!(
            !library.folder.exists(),
            "browsing must not create deck copies"
        );
    }

    #[tokio::test]
    async fn concurrent_registration_keeps_both_save_as_paths_and_deduplicates() {
        let fixture = Fixture::new();
        let first = fixture.deck("elsewhere/Deck.xml");
        let second = fixture.deck("Documents/MPC Autofill/Decks/Copy.xml");
        let library = fixture.library();
        let (a, b) = tokio::join!(library.remember(&first), library.remember(&second));
        a.unwrap();
        b.unwrap();
        let listing = library.list().await.unwrap();
        assert_eq!(
            listing.decks.len(),
            2,
            "index and folder discovery share one entry per path"
        );
        assert_eq!(read_index(&library.index).unwrap().paths.len(), 2);
    }

    #[tokio::test]
    async fn missing_files_are_skipped_but_reappear_when_available_again() {
        let fixture = Fixture::new();
        let path = fixture.deck("external/Deck.xml");
        let library = fixture.library();
        library.remember(&path).await.unwrap();
        std::fs::remove_file(&path).unwrap();
        let listing = library.list().await.unwrap();
        assert!(listing.decks.is_empty());
        assert!(listing.warning.unwrap().contains("unavailable"));
        fixture.deck("external/Deck.xml");
        let listing = fixture.library().list().await.unwrap();
        assert_eq!(listing.decks.len(), 1);
        assert!(listing.warning.is_none());
    }

    #[tokio::test]
    async fn corrupt_index_does_not_hide_managed_decks_or_get_overwritten() {
        let fixture = Fixture::new();
        let path = fixture.deck("Documents/MPC Autofill/Decks/Deck.xml");
        let library = fixture.library();
        std::fs::create_dir_all(library.index.parent().unwrap()).unwrap();
        std::fs::write(&library.index, "broken index").unwrap();
        let listing = library.list().await.unwrap();
        assert_eq!(listing.decks.len(), 1);
        assert!(listing.warning.unwrap().contains("index"));
        assert!(library.remember(&path).await.is_err());
        assert_eq!(
            std::fs::read_to_string(&library.index).unwrap(),
            "broken index"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), XML);
    }

    #[tokio::test]
    async fn default_save_location_is_user_owned_and_rejects_path_traversal() {
        let fixture = Fixture::new();
        let library = fixture.library();
        let path = library.default_path("My deck.xml").await.unwrap();
        assert_eq!(
            PathBuf::from(path),
            fixture.0.join("Documents/MPC Autofill/Decks/My deck.xml")
        );
        assert!(library.folder.is_dir());
        assert_eq!(
            library.default_path("Fix later.mpcdeck").await.unwrap(),
            fixture
                .0
                .join("Documents/MPC Autofill/Decks/Fix later.mpcdeck")
                .to_string_lossy()
        );
        assert_eq!(
            library.default_path("CON.mpcdeck").await.unwrap(),
            library.folder.join("_CON.mpcdeck").to_string_lossy()
        );
        for invalid in [
            "../escape.xml",
            "/tmp/escape.xml",
            "C:\\escape.xml",
            "deck.txt",
        ] {
            assert!(library.default_path(invalid).await.is_err());
        }
        let fallback = SavedDecks::new(fixture.0.join("data"), None);
        assert_eq!(fallback.folder, fixture.0.join("data/decks"));
    }

    #[tokio::test]
    async fn lists_a_saved_draft_even_when_its_decklist_is_invalid() {
        let fixture = Fixture::new();
        let path = fixture
            .0
            .join("Documents/MPC Autofill/Decks/Fix later.mpcdeck");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"version":1,"text":"0 Island","name":"Fix later","stock":"S30","foil":false,"choices":{},"common_back":null}"#,
        )
        .unwrap();
        let listing = fixture.library().list().await.unwrap();
        assert_eq!(listing.decks.len(), 1);
        assert!(listing.decks[0].draft);
        assert_eq!(listing.decks[0].name, "Fix later");
        assert_eq!(listing.decks[0].quantity, 0);
    }

    #[tokio::test]
    async fn sorts_by_actual_save_time_and_excludes_unusable_orders() {
        let fixture = Fixture::new();
        let older = fixture.deck("Documents/MPC Autofill/Decks/Alpha.xml");
        let newer = fixture.deck("Documents/MPC Autofill/Decks/Zeta.xml");
        for (path, seconds) in [(&older, 100), (&newer, 200)] {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(
                    std::fs::FileTimes::new()
                        .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(seconds)),
                )
                .unwrap();
        }
        let bad = fixture.deck("Documents/MPC Autofill/Decks/broken.xml");
        std::fs::write(bad, "broken").unwrap();
        let listing = fixture.library().list().await.unwrap();
        assert_eq!(
            listing
                .decks
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            ["Zeta", "Alpha"]
        );
    }
}
