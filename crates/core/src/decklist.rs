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
) -> Result<Option<Card>> {
    let face = if face == Face::Front { "front" } else { "back" };
    if let Some(selection) = selections
        .iter()
        .find(|s| s.face == face && s.query.eq_ignore_ascii_case(query))
    {
        if selection.drive_id.is_empty()
            || !selection
                .drive_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(Error::Invalid(
                "selected artwork has an invalid Google Drive ID".into(),
            ));
        }
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
            let missing = MissingArtwork { query: query.into(), face: face.into() };
            if !unresolved.iter().any(|item| item.face == missing.face && item.query.eq_ignore_ascii_case(&missing.query)) {
                unresolved.push(missing);
            }
            Ok(None)
        }
        _ => Err(Error::Invalid(format!("'{query}' matches several images. Use the full artwork name or image ID to choose one."))),
    }
}

fn parse_line(line: &str, number: usize) -> Result<(u32, &str, Option<&str>)> {
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
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
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
