//! An unfinished import is an XML deck draft, separate from a complete MPC order.
//! It preserves malformed lines and artwork choices without inventing image IDs.
use std::collections::HashMap;
use std::path::Path;

use autofill_core::decklist::ArtworkSelection;
use autofill_core::new_order::{CardbackSelection, NewOrderOptions};
use autofill_core::validation::{
    read_text_file_bounded, validate_drive_id, validate_field, MAX_XML_BYTES,
};
use autofill_core::Cardstock;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

pub const LEGACY_EXTENSION: &str = "mpcdeck";
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
        if self.name.trim().is_empty() || self.name.chars().count() > 200 {
            return Err("enter a deck name of 1–200 characters".into());
        }
        if self.choices.len() > 1_224 {
            return Err("too many artwork choices in deck draft".into());
        }
        validate_field(&self.name, "deck name").map_err(|e| e.to_string())?;
        for (key, choice) in &self.choices {
            let (query, face): (String, String) =
                serde_json::from_str(key).map_err(|_| "invalid artwork choice key")?;
            validate_field(&query, "artwork query").map_err(|e| e.to_string())?;
            if !matches!(face.as_str(), "front" | "back") {
                return Err("invalid artwork face".into());
            }
            validate_choice(choice)?;
        }
        if let Some(back) = &self.common_back {
            validate_choice(back)?;
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
        let selections = self.selections();
        match self.options().preview(&self.text, &selections) {
            Ok(preview) => {
                let mut issues: Vec<String> = preview
                    .issues
                    .iter()
                    .map(|issue| issue.message.clone())
                    .collect();
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

    pub fn selections(&self) -> Vec<ArtworkSelection> {
        self.choices
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
            .collect()
    }
}

fn validate_choice(choice: &DraftChoice) -> Result<(), String> {
    validate_drive_id(&choice.art.drive_id).map_err(|e| e.to_string())?;
    for (value, label) in [
        (&choice.art.name, "artwork name"),
        (&choice.query, "artwork query"),
        (&choice.art.label, "artwork label"),
        (&choice.art.source, "artwork source"),
        (&choice.art.thumb, "artwork thumbnail"),
    ] {
        validate_field(value, label).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(rename = "deckDraft")]
struct XmlDraft {
    version: u8,
    name: String,
    stock: Cardstock,
    foil: bool,
    text: String,
    #[serde(default)]
    choices: XmlChoices,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    common_back: Option<DraftChoice>,
}

#[derive(Default, Deserialize, Serialize)]
struct XmlChoices {
    #[serde(default, rename = "choice")]
    entries: Vec<XmlChoice>,
}

#[derive(Deserialize, Serialize)]
struct XmlChoice {
    key: String,
    value: DraftChoice,
}

pub fn is_legacy_draft(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(LEGACY_EXTENSION))
}

pub fn is_xml_draft(xml: &str) -> bool {
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag) | Event::Empty(tag)) => return tag.name().as_ref() == "deckDraft",
            Ok(Event::Eof) | Err(_) => return false,
            _ => {}
        }
    }
}

fn validate_draft_xml(xml: &str) -> Result<(), String> {
    if xml.len() as u64 > MAX_BYTES {
        return Err("choose a deck draft under 1.5 MB".into());
    }
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut depth = 0usize;
    let mut roots = 0;
    let mut elements = 0;
    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        match event {
            Event::Start(ref tag) | Event::Empty(ref tag) => {
                elements += 1;
                if depth >= 16 || elements > 30_000 || tag.name().as_ref().len() > 128 {
                    return Err("deck draft XML exceeds structural limits".into());
                }
                if depth == 0 {
                    roots += 1;
                    if roots != 1 || tag.name().as_ref() != "deckDraft" {
                        return Err("expected one deckDraft root".into());
                    }
                }
                if tag.attributes().next().is_some() {
                    return Err("deck draft XML attributes are unsupported".into());
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or("unexpected XML closing tag")?;
            }
            Event::DocType(_) | Event::PI(_) => {
                return Err("deck draft XML declarations are unsupported".into())
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if roots != 1 || depth != 0 {
        return Err("incomplete deck draft XML".into());
    }
    Ok(())
}

pub fn decode_xml(xml: &str) -> Result<DeckDraft, String> {
    validate_draft_xml(xml)?;
    if !is_xml_draft(xml) {
        return Err("not an XML deck draft".into());
    }
    let saved: XmlDraft = quick_xml::de::from_str(xml).map_err(|e| e.to_string())?;
    let draft = DeckDraft {
        version: saved.version,
        name: saved.name,
        stock: saved.stock,
        foil: saved.foil,
        text: saved.text,
        choices: saved
            .choices
            .entries
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect(),
        common_back: saved.common_back,
    };
    draft.validate()?;
    Ok(draft)
}

pub fn encode_xml(draft: &DeckDraft) -> Result<String, String> {
    draft.validate()?;
    let mut entries: Vec<_> = draft
        .choices
        .iter()
        .map(|(key, value)| XmlChoice {
            key: key.clone(),
            value: value.clone(),
        })
        .collect();
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    let saved = XmlDraft {
        version: draft.version,
        name: draft.name.clone(),
        stock: draft.stock,
        foil: draft.foil,
        text: draft.text.clone(),
        choices: XmlChoices { entries },
        common_back: draft.common_back.clone(),
    };
    let xml = quick_xml::se::to_string(&saved).map_err(|e| e.to_string())?;
    let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{xml}\n");
    if xml.len() as u64 > MAX_BYTES {
        return Err("deck draft is too large to save".into());
    }
    Ok(xml)
}

pub fn read_if_draft(path: &Path) -> Result<Option<DeckDraft>, String> {
    if !is_legacy_draft(path)
        && !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
    {
        return Ok(None);
    }
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("choose a deck file".into());
    }
    let limit = if is_legacy_draft(path) {
        MAX_BYTES as usize
    } else {
        MAX_XML_BYTES
    };
    let text = read_text_file_bounded(path, limit).map_err(|e| e.to_string())?;
    if is_legacy_draft(path) {
        if metadata.len() > MAX_BYTES {
            return Err("choose a deck draft under 1.5 MB".into());
        }
        let draft: DeckDraft = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        draft.validate()?;
        return Ok(Some(draft));
    }
    if !is_xml_draft(&text) {
        return Ok(None);
    }
    if metadata.len() > MAX_BYTES {
        return Err("choose a deck draft under 1.5 MB".into());
    }
    Ok(Some(decode_xml(&text)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_reader_rejects_oversized_deep_and_declared_xml() {
        assert!(decode_xml(&format!(
            "<deckDraft>{}</deckDraft>",
            "x".repeat(MAX_BYTES as usize)
        ))
        .is_err());
        assert!(decode_xml(&format!(
            "<deckDraft>{}{}</deckDraft>",
            "<nested>".repeat(20),
            "</nested>".repeat(20)
        ))
        .is_err());
        assert!(decode_xml("<!DOCTYPE deckDraft><deckDraft/>").is_err());
        assert!(decode_xml("<deckDraft/><deckDraft/>").is_err());
    }

    #[test]
    fn draft_validation_rejects_hostile_artwork_but_keeps_text_problems_saveable() {
        let mut draft = DeckDraft {
            version: 1,
            text: "0 Island".into(),
            name: "Needs correction".into(),
            stock: Cardstock::S30,
            foil: false,
            choices: HashMap::new(),
            common_back: None,
        };
        assert!(draft.validate().is_ok());
        assert!(draft.preview().1.unwrap().contains("quantity"));
        let choice = DraftChoice {
            art: DraftArt {
                drive_id: "../escape".into(),
                name: "Island".into(),
                label: String::new(),
                source: String::new(),
                dpi: 0,
                thumb: String::new(),
            },
            query: "Island".into(),
            index: 0,
            count: 1,
        };
        draft.common_back = Some(choice.clone());
        assert!(draft.validate().is_err());
        draft.common_back = None;
        draft
            .choices
            .insert("[\"island\",\"front\"]".into(), choice);
        assert!(draft.validate().is_err());
    }

    #[test]
    fn malformed_decklist_and_artwork_choices_survive_a_draft_round_trip() {
        let selected = DraftChoice {
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
        };
        let draft = DeckDraft {
            version: 1,
            text: "0 Island\n2 Mountain & Forest".into(),
            name: "To <fix> & finish".into(),
            stock: Cardstock::S30,
            foil: false,
            choices: HashMap::from([("[\"mountain\",\"front\"]".into(), selected.clone())]),
            common_back: Some(selected),
        };
        assert!(draft.preview().1.is_some());
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("mpc-draft-{}-{stamp}.xml", std::process::id()));
        let xml = encode_xml(&draft).unwrap();
        assert!(is_xml_draft(&xml));
        assert!(xml.contains("To &lt;fix&gt; &amp; finish"));
        assert!(!is_xml_draft("<order><details/></order>"));
        std::fs::write(&path, xml).unwrap();
        let restored = read_if_draft(&path).unwrap().unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(restored.text, draft.text);
        assert_eq!(restored.choices.len(), 1);
        assert_eq!(
            restored.choices["[\"mountain\",\"front\"]"].art.drive_id,
            "mountain-id"
        );
        assert_eq!(
            restored.common_back.as_ref().unwrap().art.drive_id,
            "mountain-id"
        );
        assert!(restored.preview().1.is_some());
    }

    #[test]
    fn older_json_drafts_can_still_be_opened() {
        let draft = DeckDraft {
            version: 1,
            text: "0 Island".into(),
            name: "Older draft".into(),
            stock: Cardstock::S30,
            foil: false,
            choices: HashMap::new(),
            common_back: None,
        };
        let path =
            std::env::temp_dir().join(format!("mpc-legacy-draft-{}.mpcdeck", std::process::id()));
        std::fs::write(&path, serde_json::to_string(&draft).unwrap()).unwrap();
        assert_eq!(read_if_draft(&path).unwrap().unwrap().text, draft.text);
        std::fs::remove_file(path).unwrap();
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
