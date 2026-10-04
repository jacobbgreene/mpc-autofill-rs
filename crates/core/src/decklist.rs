//! Text decklists retain existing art and back pairings; new art is selected explicitly.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::order::PROJECT_MAX_SIZE;
use crate::validation::{validate_drive_id, validate_field, MAX_CARD_RECORDS};
use crate::{Card, Error, Face, Order, Result, Source};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArtworkSelection {
    pub query: String,
    pub face: String,
    pub drive_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingArtwork {
    pub query: String,
    pub face: String,
    pub source_lines: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub struct RetainedArtwork {
    #[serde(flatten)]
    pub entry: MissingArtwork,
    pub card: Card,
}

#[derive(Debug, Serialize)]
pub struct DecklistIssue {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct DecklistPreview {
    pub quantity: u32,
    pub unresolved: Vec<MissingArtwork>,
    pub retained: Vec<RetainedArtwork>,
    pub common_back: Option<Card>,
    pub needs_common_back: bool,
    pub issues: Vec<DecklistIssue>,
}

struct Entry {
    quantity: u32,
    front: Option<Card>,
    back: Option<Card>,
}

fn label(card: &Card) -> String {
    card.name
        .as_deref()
        .map(|name| {
            [".png", ".jpg", ".jpeg", ".webp"]
                .iter()
                .find_map(|extension| name.strip_suffix(extension))
                .unwrap_or(name)
                .to_owned()
        })
        .unwrap_or_else(|| card.drive_id.clone())
}

fn candidates<'a>(cards: &'a [Card], query: &str) -> Vec<&'a Card> {
    let exact: Vec<_> = cards
        .iter()
        .filter(|card| {
            card.drive_id == query
                || label(card).eq_ignore_ascii_case(query)
                || card
                    .name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(query))
        })
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    cards
        .iter()
        .filter(|card| {
            card.query
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(query))
        })
        .collect()
}

fn resolve(
    cards: &[Card],
    query: &str,
    face: Face,
    selections: &[ArtworkSelection],
    unresolved: &mut Vec<MissingArtwork>,
    line: usize,
) -> Result<Option<Card>> {
    let face = if face == Face::Front { "front" } else { "back" };
    if let Some(selection) = selections
        .iter()
        .find(|s| s.face == face && s.query.eq_ignore_ascii_case(query))
    {
        validate_drive_id(&selection.drive_id)?;
        return Ok(Some(Card {
            drive_id: selection.drive_id.clone(),
            source: Source::GoogleDrive,
            slots: BTreeSet::new(),
            name: Some(selection.name.clone()),
            query: Some(query.into()),
        }));
    }
    let matches = candidates(cards, query);
    match matches.as_slice() {
        [card] => Ok(Some((*card).clone())),
        [] => {
            if let Some(item) = unresolved.iter_mut().find(|item| item.face == face && item.query.eq_ignore_ascii_case(query)) {
                item.source_lines.push(line);
            } else {
                unresolved.push(MissingArtwork { query: query.into(), face: face.into(), source_lines: vec![line] });
            }
            Ok(None)
        }
        _ => Err(Error::Invalid(format!("'{query}' matches several images. Use the full artwork name or image ID to choose one."))),
    }
}

fn is_section_heading(line: &str) -> bool {
    let Some((section, count)) = line.split_once('(') else {
        return false;
    };
    let Some(count) = count.strip_suffix(')') else {
        return false;
    };
    if count.trim().parse::<u32>().is_err() {
        return false;
    }
    matches!(
        section.trim().to_ascii_lowercase().as_str(),
        "commander"
            | "commanders"
            | "creature"
            | "creatures"
            | "artifact"
            | "artifacts"
            | "enchantment"
            | "enchantments"
            | "instant"
            | "instants"
            | "sorcery"
            | "sorceries"
            | "land"
            | "lands"
            | "planeswalker"
            | "planeswalkers"
            | "battle"
            | "battles"
            | "mainboard"
            | "sideboard"
            | "maybeboard"
            | "deck"
            | "tokens"
    )
}

fn parse_line(line: &str, number: usize) -> Result<(u32, &str, Option<&str>)> {
    validate_field(line, "decklist entry")?;
    let (count, names) = match line.split_once(char::is_whitespace) {
        Some((prefix, rest)) if prefix.chars().next().is_some_and(|c| c.is_ascii_digit()) => {
            let count: u32 = prefix.trim_end_matches(['x', 'X']).parse().map_err(|_| {
                Error::Invalid(format!("line {number}: invalid quantity '{prefix}'"))
            })?;
            (count, rest.trim())
        }
        _ => (1, line),
    };
    if count == 0 || count > PROJECT_MAX_SIZE || names.is_empty() {
        return Err(Error::Invalid(format!(
            "line {number}: enter a card name and a quantity from 1 to {PROJECT_MAX_SIZE}"
        )));
    }
    let (front, back) = names
        .split_once('|')
        .map_or((names, None), |(front, back)| {
            (front.trim(), Some(back.trim()))
        });
    if front.is_empty() || back.is_some_and(str::is_empty) {
        return Err(Error::Invalid(format!(
            "line {number}: a card name is missing"
        )));
    }
    Ok((count, front, back))
}

impl Order {
    pub fn to_decklist(&self) -> Result<String> {
        let unique_label = |card: &Card, face: &[Card]| {
            // An artwork replacement must not rename the next text draft.
            // Keep the submitted name when it still identifies this image.
            if let Some(query) = card.query.as_deref() {
                let matches = candidates(face, query);
                if matches.len() == 1
                    && matches[0].drive_id == card.drive_id
                    && matches[0].source == card.source
                {
                    return query.to_owned();
                }
            }
            let name = label(card);
            if candidates(face, &name).len() == 1 {
                name
            } else {
                card.drive_id.clone()
            }
        };
        let mut lines: Vec<(u32, String)> = Vec::new();
        for slot in 0..self.details.quantity {
            let fronts: Vec<_> = self
                .fronts
                .iter()
                .filter(|c| c.slots.contains(&slot))
                .collect();
            let [front] = fronts.as_slice() else {
                return Err(Error::Invalid(format!(
                    "card slot {} must have exactly one front before editing",
                    slot + 1
                )));
            };
            let backs: Vec<_> = self
                .backs
                .iter()
                .filter(|c| c.slots.contains(&slot))
                .collect();
            if backs.len() > 1 {
                return Err(Error::Invalid(format!(
                    "card slot {} has overlapping backs",
                    slot + 1
                )));
            }
            let mut name = unique_label(front, &self.fronts);
            let mixed_backs = self
                .backs
                .iter()
                .filter(|back| !back.slots.is_disjoint(&front.slots))
                .count()
                > 1;
            if let Some(back) = backs.first().filter(|c| {
                mixed_backs
                    || c.source != Source::GoogleDrive
                    || Some(&c.drive_id) != self.cardback_id.as_ref()
            }) {
                name.push_str(" | ");
                name.push_str(&unique_label(back, &self.backs));
            }
            // Keep slot order, including repeated art with different backs.
            if let Some((count, _)) = lines.last_mut().filter(|(_, last)| *last == name) {
                *count += 1;
            } else {
                lines.push((1, name));
            }
        }
        Ok(lines
            .into_iter()
            .map(|(count, name)| format!("{count} {name}"))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn plan_decklist(
        &self,
        text: &str,
        selections: &[ArtworkSelection],
    ) -> Result<(Vec<Entry>, DecklistPreview)> {
        let mut entries = Vec::new();
        let mut preview = DecklistPreview {
            quantity: 0,
            unresolved: Vec::new(),
            retained: Vec::new(),
            common_back: None,
            needs_common_back: false,
            issues: Vec::new(),
        };
        if text.len() > 1_000_000 {
            return Err(Error::Invalid(
                "decklist is too large; use text under 1 MB".into(),
            ));
        }
        if selections.len() > MAX_CARD_RECORDS {
            return Err(Error::Invalid("too many artwork selections".into()));
        }
        for selection in selections {
            validate_drive_id(&selection.drive_id)?;
            validate_field(&selection.name, "artwork name")?;
            validate_field(&selection.query, "artwork query")?;
            if !matches!(selection.face.as_str(), "front" | "back") {
                return Err(Error::Invalid("choose a front or back image".into()));
            }
        }
        for (index, line) in text.lines().enumerate() {
            if index >= 20_000 || entries.len() >= MAX_CARD_RECORDS {
                return Err(Error::Invalid(
                    "decklist has too many lines or entries".into(),
                ));
            }
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with("//")
                || is_section_heading(line)
            {
                continue;
            }
            let (quantity, name, back_name) = match parse_line(line, index + 1) {
                Ok(parsed) => parsed,
                Err(error) => {
                    preview.issues.push(DecklistIssue {
                        code: "invalid_line",
                        message: error.to_string(),
                        line: Some(index + 1),
                    });
                    continue;
                }
            };
            preview.quantity += quantity;
            if preview.quantity > PROJECT_MAX_SIZE
                && !preview
                    .issues
                    .iter()
                    .any(|issue| issue.code == "too_many_cards")
            {
                preview.issues.push(DecklistIssue {
                    code: "too_many_cards",
                    message: format!("decklist exceeds {PROJECT_MAX_SIZE} cards"),
                    line: Some(index + 1),
                });
            }
            // Resolve each face independently so one invalid face does not hide
            // diagnostics or artwork on the rest of the list.
            let mut resolve_face = |name, face| match resolve(
                if face == Face::Front {
                    &self.fronts
                } else {
                    &self.backs
                },
                name,
                face,
                selections,
                &mut preview.unresolved,
                index + 1,
            ) {
                Ok(card) => {
                    if let Some(card) = &card {
                        let face = if face == Face::Front { "front" } else { "back" };
                        if let Some(item) = preview.retained.iter_mut().find(|item| {
                            item.entry.face == face && item.entry.query.eq_ignore_ascii_case(name)
                        }) {
                            item.entry.source_lines.push(index + 1);
                        } else {
                            preview.retained.push(RetainedArtwork {
                                entry: MissingArtwork {
                                    query: name.into(),
                                    face: face.into(),
                                    source_lines: vec![index + 1],
                                },
                                card: card.clone(),
                            });
                        }
                    }
                    card
                }
                Err(error) => {
                    preview.issues.push(DecklistIssue {
                        code: "invalid_artwork",
                        message: error.to_string(),
                        line: Some(index + 1),
                    });
                    None
                }
            };
            let front = resolve_face(name, Face::Front);
            let back = if let Some(name) = back_name {
                resolve_face(name, Face::Back)
            } else if let Some(front) = &front {
                let associated: Vec<_> = self
                    .backs
                    .iter()
                    .filter(|back| !back.slots.is_disjoint(&front.slots))
                    .collect();
                match associated.as_slice() {
                    [back] => Some((*back).clone()),
                    [] => self.common_back(),
                    _ => {
                        preview.issues.push(DecklistIssue {
                            code: "ambiguous_back",
                            message: format!("'{name}' uses different backs. Specify each as '1 {name} | Back name'."),
                            line: Some(index + 1),
                        });
                        None
                    }
                }
            } else {
                self.common_back()
            };
            preview.needs_common_back |= back_name.is_none() && back.is_none();
            if back_name.is_none() {
                if let Some(back) = &back {
                    if back.source == Source::GoogleDrive
                        && Some(&back.drive_id) == self.cardback_id.as_ref()
                    {
                        preview.common_back = Some(back.clone());
                    }
                }
            }
            entries.push(Entry {
                quantity,
                front,
                back,
            });
        }
        if entries.is_empty() {
            preview.issues.push(DecklistIssue {
                code: "empty_deck",
                message: "decklist has no cards".into(),
                line: None,
            });
        }
        Ok((entries, preview))
    }

    fn common_back(&self) -> Option<Card> {
        let id = self.cardback_id.as_ref()?;
        Some(
            self.backs
                .iter()
                .find(|c| &c.drive_id == id && c.source == Source::GoogleDrive)
                .cloned()
                .unwrap_or_else(|| Card {
                    drive_id: id.clone(),
                    source: Source::GoogleDrive,
                    slots: BTreeSet::new(),
                    name: None,
                    query: None,
                }),
        )
    }

    pub fn preview_decklist(
        &self,
        text: &str,
        selections: &[ArtworkSelection],
    ) -> Result<DecklistPreview> {
        self.plan_decklist(text, selections)
            .map(|(_, preview)| preview)
    }

    pub fn edit_decklist(&self, text: &str, selections: &[ArtworkSelection]) -> Result<Order> {
        let (entries, preview) = self.plan_decklist(text, selections)?;
        if let Some(issue) = preview.issues.first() {
            return Err(Error::Invalid(issue.message.clone()));
        }
        if !preview.unresolved.is_empty() {
            return Err(Error::Invalid(
                "choose artwork for every new card before applying the decklist".into(),
            ));
        }
        if preview.needs_common_back {
            return Err(Error::Invalid(
                "choose a common cardback or specify a reverse for every card".into(),
            ));
        }
        let mut order = self.clone();
        order.details.quantity = preview.quantity;
        order.fronts.clear();
        order.backs.clear();
        let mut slot = 0;
        for entry in entries {
            let slots: BTreeSet<_> = (slot..slot + entry.quantity).collect();
            for (card, cards) in [
                (entry.front, &mut order.fronts),
                (entry.back, &mut order.backs),
            ] {
                if let Some(mut card) = card {
                    if let Some(existing) = cards
                        .iter_mut()
                        .find(|c| c.drive_id == card.drive_id && c.source == card.source)
                    {
                        existing.slots.extend(&slots);
                    } else {
                        card.slots = slots.clone();
                        cards.push(card);
                    }
                }
            }
            slot += entry.quantity;
        }
        order.validate_for_autofill()?;
        Ok(order)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const XML: &str = r#"<order><details><quantity>3</quantity><stock>(S30) Standard Smooth</stock><foil>false</foil></details><fronts><card><id>a</id><name>Island (Art A).png</name><query>Island</query><slots>0,2</slots></card><card><id>b</id><name>Delver.png</name><slots>1</slots></card></fronts><backs><card><id>special</id><name>Insect.png</name><slots>1</slots></card></backs><cardback>common</cardback></order>"#;
    fn order() -> Order {
        Order::from_xml("Deck", XML).unwrap()
    }

    #[test]
    fn preview_includes_retained_artwork_with_exact_lines_and_custom_backs() {
        let original = order();
        let preview = original
            .preview_decklist(
                "\u{feff}# comment\r\n2 Island\r\n0 Bad\r\n1 Delver | Insect\r\n1 island",
                &[],
            )
            .unwrap();
        assert_eq!(preview.retained.len(), 3);
        assert_eq!(preview.retained[0].entry.source_lines, [2, 5]);
        assert_eq!(preview.retained[0].card.drive_id, "a");
        assert_eq!(preview.retained[2].entry.face, "back");
        assert_eq!(preview.retained[2].entry.source_lines, [4]);
        assert_eq!(preview.common_back.unwrap().drive_id, "common");
        assert!(preview.unresolved.is_empty());
        assert_eq!(preview.issues[0].line, Some(3));
        assert_eq!(original, order());
    }

    #[test]
    fn replacement_artwork_keeps_the_submitted_name_when_reopening_text() {
        let edited = order()
            .edit_decklist(
                "2 Island",
                &[ArtworkSelection {
                    query: "Island".into(),
                    face: "front".into(),
                    drive_id: "replacement".into(),
                    name: "Mountain.png".into(),
                }],
            )
            .unwrap();
        assert_eq!(edited.to_decklist().unwrap(), "2 Island");
        let reopened = Order::from_xml("Saved", &edited.to_xml().unwrap()).unwrap();
        assert_eq!(reopened.to_decklist().unwrap(), "2 Island");
    }

    #[test]
    fn smaug_deck_ignores_category_headings_and_keeps_all_100_cards() {
        let text = include_str!("../tests/fixtures/smaug.txt");
        let preview = order().preview_decklist(text, &[]).unwrap();
        assert_eq!(preview.quantity, 100);
        assert_eq!(preview.unresolved.len(), 73);
        assert_eq!(preview.unresolved[0].query, "Smaug the Magnificent");
        let selections: Vec<_> = preview
            .unresolved
            .iter()
            .enumerate()
            .map(|(i, item)| ArtworkSelection {
                query: item.query.clone(),
                face: item.face.clone(),
                drive_id: format!("art-{i}"),
                name: item.query.clone(),
            })
            .collect();
        let edited = order().edit_decklist(text, &selections).unwrap();
        assert_eq!(edited.details.quantity, 100);
        assert_eq!(
            edited
                .fronts
                .iter()
                .find(|card| card.query.as_deref() == Some("Mountain"))
                .unwrap()
                .slots
                .len(),
            28
        );
    }

    #[test]
    fn category_detection_does_not_hide_card_names_or_artwork_suffixes() {
        for text in [
            "Smaug the Magnificent (Normal)",
            "Mountain (123)",
            "1 Lands (35)",
            "Creature Swap",
            "Mystery Category (2)",
        ] {
            assert!(!is_section_heading(text));
        }
        assert!(is_section_heading("Creatures (20)"));
        assert!(is_section_heading("COMMANDER (1)"));
    }

    #[test]
    fn unmodified_text_round_trip_preserves_every_slot_and_back() {
        let original = order();
        let edited = original
            .edit_decklist(&original.to_decklist().unwrap(), &[])
            .unwrap();
        for (face, card) in original.images() {
            let saved = edited
                .images()
                .find(|(f, c)| *f == face && c.drive_id == card.drive_id)
                .unwrap()
                .1;
            assert_eq!(saved, card);
        }
    }
    #[test]
    fn quantity_changes_and_reordering_retain_custom_backs() {
        let edited = order()
            .edit_decklist("2x Delver\n# comment\n1 Island", &[])
            .unwrap();
        assert_eq!(edited.fronts[0].slots, BTreeSet::from([0, 1]));
        assert_eq!(edited.backs[0].drive_id, "special");
        assert_eq!(edited.backs[0].slots, BTreeSet::from([0, 1]));
        assert_eq!(edited.backs[1].slots, BTreeSet::from([2]));
    }
    #[test]
    fn new_cards_require_explicit_artwork_and_use_common_back() {
        let original = order();
        assert_eq!(
            original
                .preview_decklist("2 Lightning Bolt", &[])
                .unwrap()
                .unresolved
                .len(),
            1
        );
        assert!(original.edit_decklist("2 Lightning Bolt", &[]).is_err());
        let edited = original
            .edit_decklist(
                "2 Lightning Bolt",
                &[ArtworkSelection {
                    query: "Lightning Bolt".into(),
                    face: "front".into(),
                    drive_id: "new-art".into(),
                    name: "Bolt.png".into(),
                }],
            )
            .unwrap();
        assert_eq!(edited.fronts.len(), 1);
        assert_eq!(edited.fronts[0].drive_id, "new-art");
        assert_eq!(edited.backs[0].drive_id, "common");
        assert_eq!(edited.backs[0].slots, BTreeSet::from([0, 1]));
    }
    #[test]
    fn invalid_text_never_changes_original_order() {
        let original = order();
        for text in [
            "",
            "0 Island",
            "613 Island",
            "612 Island\n1 Delver",
            "1 | Back",
            "1 Island |",
        ] {
            assert!(original.edit_decklist(text, &[]).is_err(), "{text}");
        }
        assert_eq!(original, order());
    }
    #[test]
    fn repeated_new_names_share_one_artwork_choice_regardless_of_case() {
        let preview = order()
            .preview_decklist("1 Lightning Bolt\n1 lightning bolt", &[])
            .unwrap();
        assert_eq!(preview.quantity, 2);
        assert_eq!(preview.unresolved.len(), 1);
        assert_eq!(preview.unresolved[0].source_lines, vec![1, 2]);
    }
    #[test]
    fn preview_collects_line_errors_and_keeps_valid_faces_and_source_lines() {
        let text = "\u{feff}# imported\r\nCREATURES (4)\r\n2 New Front | New Back\r\n0 Bad Quantity\r\n1 | Missing\r\n1 new front\r\n// end";
        let preview = order().preview_decklist(text, &[]).unwrap();
        assert_eq!(preview.quantity, 3);
        assert_eq!(
            preview
                .issues
                .iter()
                .map(|issue| issue.line)
                .collect::<Vec<_>>(),
            vec![Some(4), Some(5)]
        );
        assert_eq!(preview.unresolved.len(), 2);
        assert_eq!(preview.unresolved[0].source_lines, vec![3, 6]);
        assert_eq!(preview.unresolved[1].source_lines, vec![3]);
        assert_eq!(preview.unresolved[1].face, "back");
        assert!(order().edit_decklist(text, &[]).is_err());
    }
    #[test]
    fn partial_previews_cannot_bypass_deck_limit_or_invalid_text() {
        let original = order();
        let preview = original
            .preview_decklist("612 Island\n1 Delver\n0 Bad", &[])
            .unwrap();
        assert_eq!(preview.quantity, 613);
        assert!(preview
            .issues
            .iter()
            .any(|issue| issue.code == "too_many_cards"));
        assert!(original.edit_decklist("612 Island\n1 Delver", &[]).is_err());
        assert!(original.edit_decklist("1 Island\n0 Bad", &[]).is_err());
        assert_eq!(original, order());
        assert!(original
            .preview_decklist("612 Island", &[])
            .unwrap()
            .issues
            .is_empty());
    }

    #[test]
    fn hostile_selections_and_oversized_entry_lists_leave_original_unchanged() {
        let original = order();
        for id in [
            "../escape".to_owned(),
            "art#/../../escape".to_owned(),
            "a".repeat(257),
        ] {
            let selection = ArtworkSelection {
                query: "New Card".into(),
                face: "front".into(),
                drive_id: id,
                name: "New Card.png".into(),
            };
            assert!(original
                .preview_decklist("1 New Card", &[selection.clone()])
                .is_err());
            assert!(original.edit_decklist("1 New Card", &[selection]).is_err());
        }
        assert!(original
            .preview_decklist(&"1 Island\n".repeat(MAX_CARD_RECORDS + 1), &[])
            .is_err());
        assert_eq!(original, order());
    }
    #[test]
    fn non_ascii_names_are_not_conflated() {
        let preview = order().preview_decklist("1 Æther\n1 æther", &[]).unwrap();
        assert_eq!(preview.unresolved.len(), 2);
    }
    #[test]
    fn multiple_backs_for_one_front_round_trip_with_explicit_names() {
        let original = Order::from_xml(
            "Mixed",
            &XML.replace(
                "<slots>1</slots></card></backs>",
                "<slots>0,1</slots></card></backs>",
            ),
        )
        .unwrap();
        let text = original.to_decklist().unwrap();
        let edited = original.edit_decklist(&text, &[]).unwrap();
        assert_eq!(
            edited
                .backs
                .iter()
                .find(|c| c.drive_id == "special")
                .unwrap()
                .slots,
            BTreeSet::from([0, 1])
        );
        assert_eq!(
            edited
                .backs
                .iter()
                .find(|c| c.drive_id == "common")
                .unwrap()
                .slots,
            BTreeSet::from([2])
        );
    }
}
