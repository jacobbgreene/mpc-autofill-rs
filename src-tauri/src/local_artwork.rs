//! Session grants never come from serialized frontend state. Only the native
//! confirmation/picker in Session supplies the decision to `review`.
use autofill_core::{Order, Source};
use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
};

pub struct ReviewedLocalImage {
    pub reference: String,
}

pub struct LocalArtwork {
    directory: tempfile::TempDir,
    base: PathBuf,
    grants: HashMap<String, PathBuf>,
}

/// Choosing a file already used by this deck merges its placements, so the UI
/// and uploader still have one entry per source and image on each face.
pub fn replace_reference(order: &Order, old: &str, new: &str) -> Result<Order, String> {
    let mut edited = order.clone();
    for face in [&mut edited.fronts, &mut edited.backs] {
        let mut merged: Vec<autofill_core::Card> = Vec::new();
        for mut card in face.drain(..) {
            if card.source == Source::LocalFile && card.drive_id == old {
                card.drive_id = new.to_owned();
            }
            if let Some(existing) = merged
                .iter_mut()
                .find(|other| other.source == card.source && other.drive_id == card.drive_id)
            {
                existing.slots.extend(card.slots);
            } else {
                merged.push(card);
            }
        }
        *face = merged;
    }
    edited.validate().map_err(|e| e.to_string())?;
    Ok(edited)
}

impl LocalArtwork {
    pub fn new(cache: &Path, order_path: &Path) -> Result<Self, String> {
        let root = cache.join("local-sessions");
        autofill_core::cache::private_directory(&root)?;
        let directory = tempfile::Builder::new()
            .prefix("approved-")
            .tempdir_in(root)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            directory,
            base: order_path.parent().unwrap_or(Path::new("")).to_path_buf(),
            grants: HashMap::new(),
        })
    }

    pub fn get(&self, reference: &str) -> Option<&PathBuf> {
        self.grants.get(reference)
    }

    // This only manipulates path strings. It must never probe the original.
    pub fn resolve(&self, reference: &str) -> PathBuf {
        self.base.join(reference)
    }

    /// The closure is supplied by Rust's native dialog/picker adapter, never by
    /// IPC state. No source file operation occurs before it returns approval.
    pub async fn review<F, Fut>(
        &mut self,
        order: &Order,
        reference: &str,
        replace: bool,
        decide: F,
    ) -> Result<Option<ReviewedLocalImage>, String>
    where
        F: FnOnce(PathBuf) -> Fut,
        Fut: Future<Output = Result<Option<PathBuf>, String>>,
    {
        order.validate().map_err(|error| error.to_string())?;
        if !order
            .images()
            .any(|(_, card)| card.source == Source::LocalFile && card.drive_id == reference)
        {
            return Err("this local image is not part of the loaded deck".into());
        }
        if !replace && self.get(reference).is_some() {
            return Ok(None);
        }
        let resolved = self.resolve(reference);
        let Some(source) = decide(resolved.clone()).await? else {
            return Ok(None);
        };
        if !replace && source != resolved {
            return Err("the approved image path does not match the reviewed reference".into());
        }
        let reference = if replace {
            source
                .to_str()
                .ok_or("image path must be valid Unicode")?
                .to_owned()
        } else {
            reference.to_owned()
        };
        autofill_core::validation::validate_field(&reference, "local image path")
            .map_err(|error| error.to_string())?;
        self.approve(&reference, &source).await?;
        Ok(Some(ReviewedLocalImage { reference }))
    }

    /// Build the complete authorization result before the actor creates any
    /// network job or browser. A partial set of grants is never a success.
    pub fn approved_paths(
        &self,
        order: &Order,
    ) -> Result<HashMap<(Source, String), PathBuf>, String> {
        order
            .validate_for_autofill()
            .map_err(|error| error.to_string())?;
        let mut paths = HashMap::new();
        for (_, card) in order
            .images()
            .filter(|(_, card)| card.source == Source::LocalFile)
        {
            let path = self
                .get(&card.drive_id)
                .ok_or("Review local images before starting autofill")?;
            paths.insert((Source::LocalFile, card.drive_id.clone()), path.clone());
        }
        Ok(paths)
    }

    async fn approve(&mut self, reference: &str, source: &Path) -> Result<PathBuf, String> {
        let dest = self.directory.path().join(autofill_core::cache::image_key(
            "local",
            reference,
            "approved-original-v1",
        ));
        let path = autofill_core::local_files::snapshot_approved(source, &dest).await?;
        self.grants.insert(reference.to_owned(), path.clone());
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(references: &[&str]) -> Order {
        use autofill_core::{Card, Cardstock, Details};
        Order {
            name: "local approval fixture".into(),
            details: Details {
                quantity: references.len() as u32,
                stock: Cardstock::S30,
                foil: false,
            },
            fronts: references
                .iter()
                .enumerate()
                .map(|(slot, reference)| Card {
                    drive_id: (*reference).into(),
                    source: Source::LocalFile,
                    slots: [slot as u32].into_iter().collect(),
                    name: None,
                    query: None,
                })
                .collect(),
            backs: vec![Card {
                drive_id: "common".into(),
                source: Source::GoogleDrive,
                slots: (0..references.len() as u32).collect(),
                name: None,
                query: None,
            }],
            cardback_id: Some("common".into()),
        }
    }

    fn png() -> &'static [u8] {
        &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 96, 96,
            96, 0, 0, 0, 4, 0, 1, 246, 23, 56, 85, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ]
    }

    #[test]
    fn replacing_with_an_existing_local_image_merges_only_matching_sources() {
        let order = Order::from_xml("local", r#"<order><details><quantity>3</quantity><stock>(S30) Standard Smooth</stock><foil>false</foil></details><fronts><card><id>old</id><sourceType>Local File</sourceType><slots>0</slots></card><card><id>chosen</id><sourceType>Local File</sourceType><slots>1</slots></card><card><id>chosen</id><slots>2</slots></card></fronts><cardback>back</cardback></order>"#).unwrap();
        let edited = replace_reference(&order, "old", "chosen").unwrap();
        assert_eq!(edited.fronts.len(), 2);
        assert_eq!(edited.fronts[0].slots, [0, 1].into_iter().collect());
        assert_eq!(edited.fronts[0].source, Source::LocalFile);
        assert_eq!(edited.fronts[1].source, Source::GoogleDrive);
        assert_eq!(edited.fronts[1].slots, [2].into_iter().collect());
        assert_eq!(order.fronts[0].drive_id, "old");
        assert_eq!(
            Order::from_xml("local", &edited.to_xml().unwrap()).unwrap(),
            edited
        );
    }

    #[test]
    fn loading_does_not_grant_or_probe_references_and_new_session_has_no_grants() {
        let cache = tempfile::tempdir().unwrap();
        let session = LocalArtwork::new(cache.path(), Path::new("decks/order.xml")).unwrap();
        assert!(session.get("nonexistent.png").is_none());
        assert_eq!(
            session.resolve("nonexistent.png"),
            PathBuf::from("decks/nonexistent.png")
        );
        assert!(std::fs::read_dir(session.directory.path())
            .unwrap()
            .next()
            .is_none());
        let replacement =
            LocalArtwork::new(cache.path(), Path::new("elsewhere/order.xml")).unwrap();
        assert!(replacement.get("nonexistent.png").is_none());
        assert_ne!(session.directory.path(), replacement.directory.path());
    }

    #[tokio::test]
    async fn approved_copy_survives_original_change_and_failed_approval_grants_nothing() {
        let cache = tempfile::tempdir().unwrap();
        let original = cache.path().join("original.png");
        // Known 1x1 RGB PNG; no external files are read by this test.
        let png = [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 96, 96,
            96, 0, 0, 0, 4, 0, 1, 246, 23, 56, 85, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];
        std::fs::write(&original, png).unwrap();
        let mut session = LocalArtwork::new(cache.path(), &cache.path().join("order.xml")).unwrap();
        let deck = order(&["original.png", "invalid.png"]);
        let reviewed = session
            .review(&deck, "original.png", false, |path| async {
                Ok(Some(path))
            })
            .await
            .unwrap()
            .unwrap();
        let copy = session.get(&reviewed.reference).unwrap().clone();
        assert_eq!(reviewed.reference, "original.png");
        std::fs::write(&original, b"replaced after approval").unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), png);
        std::fs::write(cache.path().join("invalid.png"), "not an image").unwrap();
        assert!(session
            .review(&deck, "invalid.png", false, |path| async { Ok(Some(path)) })
            .await
            .is_err());
        assert!(session.get("invalid.png").is_none());
        assert!(
            session.approved_paths(&deck).is_err(),
            "partial approval cannot produce upload jobs"
        );
        drop(session);
        assert!(!copy.exists());
        assert!(original.exists());
    }

    #[tokio::test]
    async fn denial_picker_cancel_and_unknown_references_never_grant_or_probe() {
        let cache = tempfile::tempdir().unwrap();
        let mut session = LocalArtwork::new(cache.path(), &cache.path().join("order.xml")).unwrap();
        let deck = order(&["missing.png"]);
        for replace in [false, true] {
            let result = session
                .review(&deck, "missing.png", replace, |path| async move {
                    assert!(!path.exists());
                    Ok(None)
                })
                .await
                .unwrap();
            assert!(result.is_none());
        }
        let called = std::cell::Cell::new(false);
        assert!(session
            .review(&deck, "unlisted.png", false, |_| {
                called.set(true);
                std::future::ready(Ok(None))
            })
            .await
            .is_err());
        assert!(!called.get());
        assert!(session.approved_paths(&deck).is_err());
        assert!(std::fs::read_dir(session.directory.path())
            .unwrap()
            .next()
            .is_none());
        assert!(!cache.path().join("missing.png").exists());
    }

    #[tokio::test]
    async fn native_dialog_failure_and_pending_cancellation_leave_no_grants() {
        let cache = tempfile::tempdir().unwrap();
        let mut session = LocalArtwork::new(cache.path(), &cache.path().join("order.xml")).unwrap();
        let deck = order(&["private.png"]);
        assert!(session
            .review(&deck, "private.png", false, |_| async {
                Err("native dialog unavailable".into())
            })
            .await
            .is_err());
        let called = std::cell::Cell::new(false);
        {
            let mut review = Box::pin(session.review(&deck, "private.png", false, |_| {
                called.set(true);
                std::future::pending::<Result<Option<PathBuf>, String>>()
            }));
            assert!(futures::poll!(review.as_mut()).is_pending());
            // Actor shutdown cancels this exact future; a late native callback
            // cannot resume it or produce a grant.
            drop(review);
        }
        assert!(called.get());
        assert!(session.get("private.png").is_none());
        assert!(session.approved_paths(&deck).is_err());
        assert!(std::fs::read_dir(session.directory.path())
            .unwrap()
            .next()
            .is_none());
    }

    #[tokio::test]
    async fn native_approval_precedes_file_read_and_reload_revokes_it() {
        let cache = tempfile::tempdir().unwrap();
        let path = cache.path().join("order.xml");
        let mut session = LocalArtwork::new(cache.path(), &path).unwrap();
        let deck = order(&["chosen.png"]);
        let reviewed = session
            .review(&deck, "chosen.png", false, |source| async move {
                // A source read before the trusted decision would fail: the file
                // only comes into existence inside the decision adapter.
                assert!(!source.exists());
                std::fs::write(&source, png()).unwrap();
                Ok(Some(source))
            })
            .await
            .unwrap()
            .unwrap();
        let paths = session.approved_paths(&deck).unwrap();
        let snapshot = session.get(&reviewed.reference).unwrap().clone();
        assert_eq!(paths[&(Source::LocalFile, "chosen.png".into())], snapshot);
        assert_eq!(std::fs::read(&snapshot).unwrap(), png());
        assert!(session
            .review(&deck, "chosen.png", false, |_| async {
                panic!("an existing session approval should not reopen the native dialog")
            })
            .await
            .unwrap()
            .is_none());
        let replacement = LocalArtwork::new(cache.path(), &path).unwrap();
        assert!(replacement.approved_paths(&deck).is_err());
        assert_eq!(
            std::fs::read(cache.path().join("chosen.png")).unwrap(),
            png()
        );
    }

    #[tokio::test]
    async fn replacement_grants_only_the_native_picker_result() {
        let cache = tempfile::tempdir().unwrap();
        let mut session = LocalArtwork::new(cache.path(), &cache.path().join("order.xml")).unwrap();
        let deck = order(&["declared.png"]);
        let chosen = cache.path().join("picked.png");
        std::fs::write(&chosen, png()).unwrap();
        // A confirmation cannot approve a different path than the one shown.
        assert!(session
            .review(&deck, "declared.png", false, |_| async {
                Ok(Some(chosen.clone()))
            })
            .await
            .is_err());
        let reviewed = session
            .review(&deck, "declared.png", true, |_| async {
                Ok(Some(chosen.clone()))
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reviewed.reference, chosen.to_str().unwrap());
        assert!(session.get("declared.png").is_none());
        assert!(session.approved_paths(&deck).is_err());
        let replaced = order(&[reviewed.reference.as_str()]);
        assert_eq!(session.approved_paths(&replaced).unwrap().len(), 1);
        assert_eq!(std::fs::read(chosen).unwrap(), png());
    }
}
