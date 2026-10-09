//! Text decklists retain existing art and back pairings; new art is selected explicitly.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::order::PROJECT_MAX_SIZE;
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
}

#[derive(Debug, Serialize)]
pub struct DecklistPreview {
    pub quantity: u32,
    pub unresolved: Vec<MissingArtwork>,
    pub needs_common_back: bool,
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
            name.rsplit_once('.')
                .filter(|(_, extension)| {
                    ["png", "jpg", "jpeg", "webp"]
                        .iter()
                        .any(|known| extension.eq_ignore_ascii_case(known))
                })
                .map(|(stem, _)| stem)
                .unwrap_or(name)
                .to_owned()
        })
        .unwrap_or_else(|| card.drive_id.clone())
}

fn candidates<'a>(cards: &'a [Card], query: &str) -> Vec<&'a Card> {
    let ids: Vec<_> = cards.iter().filter(|card| card.drive_id == query).collect();
    if !ids.is_empty() {
        return ids;
    }
    let exact: Vec<_> = cards
        .iter()
        .filter(|card| {
            label(card).eq_ignore_ascii_case(query)
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
) -> Result<Option<Card>> {
    let face = if face == Face::Front { "front" } else { "back" };
    if let Some(selection) = selections
        .iter()
        .find(|s| s.face == face && s.query.eq_ignore_ascii_case(query))
    {
        if !crate::order::valid_drive_id(&selection.drive_id) {
            return Err(Error::Invalid(
                "selected artwork has an invalid Google Drive ID".into(),
            ));
        }
        let mut selected = Card {
            drive_id: selection.drive_id.clone(),
            source: Source::GoogleDrive,
            slots: BTreeSet::new(),
            name: Some(selection.name.clone()),
            query: Some(query.into()),
        };
        if let Some(existing) = cards
            .iter()
            .find(|card| card.drive_id == selection.drive_id && card.source == Source::GoogleDrive)
        {
            selected.slots = existing.slots.clone();
        }
        return Ok(Some(selected));
    }
    let matches = candidates(cards, query);
    match matches.as_slice() {
        [card] => Ok(Some((*card).clone())),
        [] => {
            let missing = MissingArtwork { query: query.into(), face: face.into() };
            if !unresolved.iter().any(|item| item.face == missing.face && item.query.eq_ignore_ascii_case(&missing.query)) {
                unresolved.push(missing);
            }
            Ok(None)
        }
        _ => Err(Error::Invalid(format!("'{query}' matches several images. Use the full artwork name or image ID to choose one."))),
    }
}

fn is_section_heading(line: &str) -> bool {
    let line = line.trim().trim_end_matches(':').trim();
    let section = if let Some((section, count)) = line.split_once('(') {
        let Some(count) = count.strip_suffix(')') else {
            return false;
        };
        if count.trim().parse::<u32>().is_err() {
            return false;
        }
        section.trim()
    } else {
        line
    };
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
    let (count, names) = match line.split_once(char::is_whitespace) {
        Some((prefix, rest)) if prefix.chars().next().is_some_and(|c| c.is_ascii_digit()) => {
            let count: u32 = prefix
                .strip_suffix(['x', 'X'])
                .unwrap_or(prefix)
                .parse()
                .map_err(|_| {
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
        .or_else(|| names.split_once("//"))
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
            let name = label(card);
            if !name.contains('|')
                && !name.contains("//")
                && !name.contains(['\r', '\n'])
                && name.trim() == name
                && matches!(candidates(face, &name).as_slice(), [found] if found.drive_id == card.drive_id && found.source == card.source)
            {
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
            if let Some(back) = backs
                .first()
                .filter(|c| mixed_backs || Some(&c.drive_id) != self.cardback_id.as_ref())
            {
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
            needs_common_back: false,
        };
        for (index, line) in text.lines().enumerate() {
            let line = line.trim().trim_start_matches('\u{feff}').trim();
            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with("//")
                || is_section_heading(line)
            {
                continue;
            }
            let (quantity, name, back_name) = parse_line(line, index + 1)?;
            preview.quantity += quantity;
            if preview.quantity > PROJECT_MAX_SIZE {
                return Err(Error::Invalid(format!(
                    "decklist exceeds {PROJECT_MAX_SIZE} cards"
                )));
            }
            let front = resolve(
                &self.fronts,
                name,
                Face::Front,
                selections,
                &mut preview.unresolved,
            )?;
            let back = if let Some(name) = back_name {
                resolve(
                    &self.backs,
                    name,
                    Face::Back,
                    selections,
                    &mut preview.unresolved,
                )?
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
                        return Err(Error::Invalid(format!(
                        "'{name}' uses different backs. Specify each as '1 {name} | Back name'."
                    )))
                    }
                }
            } else {
                self.common_back()
            };
            preview.needs_common_back |= back_name.is_none() && back.is_none();
            entries.push(Entry {
                quantity,
                front,
                back,
            });
        }
        if entries.is_empty() {
            return Err(Error::Invalid("decklist has no cards".into()));
        }
        Ok((entries, preview))
    }

    fn common_back(&self) -> Option<Card> {
        let id = self.cardback_id.as_ref()?;
        Some(
            self.backs
                .iter()
                .find(|c| &c.drive_id == id)
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
        if !preview.unresolved.is_empty() {
            return Err(Error::Invalid(
                "choose artwork for every new card before applying the decklist".into(),
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
                    if let Some(existing) = cards.iter_mut().find(|c| c.drive_id == card.drive_id) {
                        existing.slots.extend(&slots);
                    } else {
                        card.slots = slots.clone();
                        cards.push(card);
                    }
                }
            }
            slot += entry.quantity;
        }
        order.validate()?;
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
            "1 Sideboard",
        ] {
            assert!(!is_section_heading(text));
        }
        assert!(is_section_heading("Creatures (20)"));
        assert!(is_section_heading("COMMANDER (1)"));
        assert!(is_section_heading("Sideboard"));
        assert!(is_section_heading("SIDEBOARD:"));
        assert!(is_section_heading("Sideboard (15)"));
    }

    #[test]
    fn bare_sideboard_heading_is_not_a_card_or_artwork_request() {
        let preview = order()
            .preview_decklist("Sideboard\n1 Island\nSIDEBOARD:\n1 Delver", &[])
            .unwrap();
        assert_eq!(preview.quantity, 2);
        assert!(preview.unresolved.is_empty());
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
    fn replacement_keeps_front_art_when_a_custom_back_is_added() {
        let original = order();
        let selections = [ArtworkSelection {
            query: "Insectile Aberration".into(),
            face: "back".into(),
            drive_id: "new-insect".into(),
            name: "Insectile Aberration.png".into(),
        }];
        let edited = original
            .edit_decklist("2 Island\n1 Delver | Insectile Aberration", &selections)
            .unwrap();
        assert_eq!(
            edited
                .fronts
                .iter()
                .find(|card| card.drive_id == "b")
                .unwrap()
                .slots,
            BTreeSet::from([2])
        );
        assert_eq!(
            edited
                .backs
                .iter()
                .find(|card| card.drive_id == "new-insect")
                .unwrap()
                .slots,
            BTreeSet::from([2])
        );
        assert_eq!(
            edited
                .backs
                .iter()
                .find(|card| card.drive_id == "common")
                .unwrap()
                .slots,
            BTreeSet::from([0, 1])
        );
    }
    #[test]
    fn double_slash_marks_a_back_without_losing_matching_front_art() {
        let original = order();
        let preview = original
            .preview_decklist("// note\n1 Delver // Insectile Aberration", &[])
            .unwrap();
        assert_eq!(preview.unresolved.len(), 1);
        assert_eq!(preview.unresolved[0].query, "Insectile Aberration");
        assert_eq!(preview.unresolved[0].face, "back");
        let edited = original
            .edit_decklist(
                "// note\n1 Delver // Insectile Aberration",
                &[ArtworkSelection {
                    query: "Insectile Aberration".into(),
                    face: "back".into(),
                    drive_id: "new-insect".into(),
                    name: "Insectile Aberration.png".into(),
                }],
            )
            .unwrap();
        assert_eq!(edited.fronts[0].drive_id, "b");
        assert_eq!(edited.backs[0].drive_id, "new-insect");
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
            "2xx Island",
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
    }
    #[test]
    fn full_image_id_disambiguates_a_name_that_matches_another_image_id() {
        let original =
            Order::from_xml("Deck", &XML.replace("Island (Art A).png", "b.png")).unwrap();
        let edited = original.edit_decklist("1 b", &[]).unwrap();
        assert_eq!(edited.fronts[0].drive_id, "b");
        let round_trip = original
            .edit_decklist(&original.to_decklist().unwrap(), &[])
            .unwrap();
        assert_eq!(round_trip.fronts, original.fronts);
    }

    #[test]
    fn artwork_names_with_decklist_delimiters_round_trip_using_image_ids() {
        for name in ["Fire // Ice.png", "Fire | Ice.png", "Fire\nIce.png"] {
            let original =
                Order::from_xml("Deck", &XML.replace("Island (Art A).png", name)).unwrap();
            let edited = original
                .edit_decklist(&original.to_decklist().unwrap(), &[])
                .unwrap();
            for (face, card) in original.images() {
                assert!(edited
                    .images()
                    .any(|(saved_face, saved)| saved_face == face && saved == card));
            }
        }
    }

    #[test]
    fn selecting_an_existing_front_retains_its_custom_back() {
        let original = order();
        let edited = original
            .edit_decklist(
                "2 Delver",
                &[ArtworkSelection {
                    query: "Delver".into(),
                    face: "front".into(),
                    drive_id: "b".into(),
                    name: "Delver.png".into(),
                }],
            )
            .unwrap();
        assert_eq!(edited.backs[0].drive_id, "special");
        assert_eq!(edited.backs[0].slots, BTreeSet::from([0, 1]));
    }

    #[test]
    fn uppercase_image_extensions_and_bom_whitespace_resolve_existing_art() {
        let original = Order::from_xml("Deck", &XML.replace("Delver.png", "Delver.PNG")).unwrap();
        let edited = original.edit_decklist("\u{feff}  2 Delver", &[]).unwrap();
        assert_eq!(edited.fronts[0].drive_id, "b");
        assert_eq!(edited.details.quantity, 2);
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
