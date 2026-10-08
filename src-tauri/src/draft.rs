//! An unfinished text import is kept separately from a MakePlayingCards order.
//! This preserves malformed lines and artwork choices without inventing image IDs.
use std::collections::HashMap;
use std::path::Path;

use autofill_core::decklist::ArtworkSelection;
use autofill_core::new_order::{CardbackSelection, NewOrderOptions};
use autofill_core::Cardstock;
use serde::{Deserialize, Serialize};

pub const EXTENSION: &str = "mpcdeck";
const MAX_BYTES: u64 = 1_500_000;

#[derive(Clone, Deserialize, Serialize)]
pub struct DeckDraft {
    pub version: u8,
    pub text: String,
    pub name: String,
    pub stock: Cardstock,
    pub foil: bool,
    pub choices: HashMap<String, DraftChoice>,
    pub common_back: Option<DraftChoice>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct DraftChoice {
    pub art: DraftArt,
    pub query: String,
    pub index: usize,
    pub count: usize,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct DraftArt {
    pub drive_id: String,
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub dpi: u32,
    #[serde(default)]
    pub thumb: String,
}

impl DeckDraft {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported deck draft version".into());
        }
        if self.text.trim().is_empty() || self.text.len() > 1_000_000 {
            return Err("enter a decklist under 1 MB before saving a draft".into());
        }
        if self.name.trim().is_empty() || self.name.len() > 200 {
            return Err("enter a deck name of 1–200 characters".into());
        }
        if self.stock == Cardstock::P10 && self.foil {
            return Err("plastic cardstock cannot be foil".into());
        }
        if self.choices.len() > 1_224 {
            return Err("too many artwork choices in deck draft".into());
        }
        Ok(())
    }

    pub fn options(&self) -> NewOrderOptions {
        NewOrderOptions {
            name: self.name.clone(),
            stock: self.stock,
            foil: self.foil,
            cardback: self.common_back.as_ref().map(|choice| CardbackSelection {
                drive_id: choice.art.drive_id.clone(),
                name: choice.art.name.clone(),
            }),
        }
    }

    pub fn preview(&self) -> (u32, Option<String>) {
        let selections: Vec<_> = self
            .choices
            .iter()
            .filter_map(|(key, choice)| {
                let (query, face): (String, String) = serde_json::from_str(key).ok()?;
                Some(ArtworkSelection {
                    query,
                    face,
                    drive_id: choice.art.drive_id.clone(),
                    name: choice.art.name.clone(),
                })
            })
            .collect();
        match self.options().preview(&self.text, &selections) {
            Ok(preview) => {
                let mut issues = Vec::new();
                if !preview.unresolved.is_empty() {
                    issues.push(format!(
                        "{} image{} still {} artwork",
                        preview.unresolved.len(),
                        if preview.unresolved.len() == 1 {
                            ""
                        } else {
                            "s"
                        },
                        if preview.unresolved.len() == 1 {
                            "needs"
                        } else {
                            "need"
                        }
                    ));
                }
                if preview.needs_common_back {
                    issues.push("a common card back is still needed".to_owned());
                }
                let issue = (!issues.is_empty()).then(|| issues.join("; "));
                (preview.quantity, issue)
            }
            Err(error) => (0, Some(error.to_string())),
        }
    }
}

pub fn is_draft(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION))
}

pub fn read(path: &Path) -> Result<DeckDraft, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("choose a deck draft under 1.5 MB".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let draft: DeckDraft = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    draft.validate()?;
    Ok(draft)
}

pub fn encode(draft: &DeckDraft) -> Result<String, String> {
    draft.validate()?;
    let text = serde_json::to_string_pretty(draft).map_err(|e| e.to_string())?;
    if text.len() as u64 > MAX_BYTES {
        return Err("deck draft is too large to save".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_decklist_and_artwork_choices_survive_a_draft_round_trip() {
        let draft = DeckDraft {
            version: 1,
            text: "0 Island\n2 Mountain".into(),
            name: "To fix".into(),
            stock: Cardstock::S30,
            foil: false,
            choices: HashMap::from([(
                "[\"mountain\",\"front\"]".into(),
                DraftChoice {
                    art: DraftArt {
                        drive_id: "mountain-id".into(),
                        name: "Mountain.png".into(),
                        label: "Mountain".into(),
                        source: "MPC Autofill".into(),
                        dpi: 800,
                        thumb: String::new(),
                    },
                    query: "Mountain".into(),
                    index: 0,
                    count: 3,
                },
            )]),
            common_back: None,
        };
        assert!(draft.preview().1.is_some());
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("mpc-draft-{}-{stamp}.mpcdeck", std::process::id()));
        std::fs::write(&path, encode(&draft).unwrap()).unwrap();
        let restored = read(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(restored.text, draft.text);
        assert_eq!(restored.choices.len(), 1);
        assert_eq!(
            restored.choices["[\"mountain\",\"front\"]"].art.drive_id,
            "mountain-id"
        );
        assert!(restored.preview().1.is_some());
    }

    #[test]
    fn preview_counts_only_artwork_that_is_still_missing() {
        let art = DraftArt {
            drive_id: "mountain-id".into(),
            name: "Mountain.png".into(),
            label: "Mountain".into(),
            source: String::new(),
            dpi: 0,
            thumb: String::new(),
        };
        let choice = DraftChoice {
            art: art.clone(),
            query: "Mountain".into(),
            index: 0,
            count: 1,
        };
        let mut draft = DeckDraft {
            version: 1,
            text: "2 Mountain\n1 Island".into(),
            name: "Finish later".into(),
            stock: Cardstock::S30,
            foil: false,
            choices: HashMap::from([("[\"mountain\",\"front\"]".into(), choice.clone())]),
            common_back: None,
        };
        let (quantity, issue) = draft.preview();
        assert_eq!(quantity, 3);
        let issue = issue.unwrap();
        assert!(issue.contains("1 image still needs artwork"));
        assert!(issue.contains("common card back"));
        draft
            .choices
            .insert("[\"island\",\"front\"]".into(), choice.clone());
        draft.common_back = Some(choice);
        assert_eq!(draft.preview(), (3, None));
    }
}
