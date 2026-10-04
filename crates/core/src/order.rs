//! The card-order data model and its XML parser.
//!
//! Mirrors the semantics of the reference Python tool's `order.py`: cards sharing
//! a Drive id within a face are merged, and any back slots left uncovered are
//! filled with the order's common cardback.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use crate::validation::{
    read_text_file_bounded, validate_drive_id, validate_field, validate_xml, MAX_XML_BYTES,
};
use crate::{Error, Result};

/// The largest project a MakePlayingCards family site accepts.
pub const PROJECT_MAX_SIZE: u32 = 612;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Face {
    Front,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum Source {
    GoogleDrive,
    LocalFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Cardstock {
    S27,
    S30,
    S33,
    M31,
    P10,
}

impl Cardstock {
    /// The exact label the site's cardstock dropdown displays, which is also how
    /// orders encode the stock in XML.
    pub fn label(self) -> &'static str {
        match self {
            Cardstock::S27 => "(S27) Smooth",
            Cardstock::S30 => "(S30) Standard Smooth",
            Cardstock::S33 => "(S33) Superior Smooth",
            Cardstock::M31 => "(M31) Linen",
            Cardstock::P10 => "(P10) Plastic",
        }
    }

    fn parse(label: &str) -> Option<Cardstock> {
        [
            Cardstock::S27,
            Cardstock::S30,
            Cardstock::S33,
            Cardstock::M31,
            Cardstock::P10,
        ]
        .into_iter()
        .find(|stock| stock.label() == label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Details {
    pub quantity: u32,
    pub stock: Cardstock,
    pub foil: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Card {
    pub drive_id: String,
    pub source: Source,
    pub slots: BTreeSet<u32>,
    pub name: Option<String>,
    pub query: Option<String>,
}

impl Card {
    /// The on-disk file name for this card's downloaded image. Falls back to a
    /// `.png` assumption when the order omits a name, matching the Python tool.
    pub fn file_name(&self) -> String {
        match &self.name {
            Some(name) => sanitize_filename::sanitize(name),
            None => sanitize_filename::sanitize(format!("{}.png", self.drive_id)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Order {
    pub name: String,
    pub details: Details,
    pub fronts: Vec<Card>,
    pub backs: Vec<Card>,
    /// The order's common cardback Drive id, if it declared one.
    pub cardback_id: Option<String>,
}

impl Order {
    pub async fn from_file(path: impl AsRef<Path>) -> Result<Order> {
        let path = path.as_ref();
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Project")
            .to_owned();
        let path = path.to_owned();
        let xml = tokio::task::spawn_blocking(move || read_text_file_bounded(path, MAX_XML_BYTES))
            .await
            .map_err(|error| Error::Invalid(format!("could not read order: {error}")))??;
        Order::from_xml(&name, &xml)
    }

    pub fn from_xml(name: &str, xml: &str) -> Result<Order> {
        validate_xml(xml)?;
        let raw: RawOrder = quick_xml::de::from_str(xml)?;

        let quantity: u32 = raw.details.quantity.trim().parse().map_err(|_| {
            Error::Invalid(format!(
                "quantity '{}' is not a number",
                raw.details.quantity
            ))
        })?;
        validate_quantity(quantity)?;
        let stock = Cardstock::parse(raw.details.stock.trim()).ok_or_else(|| {
            Error::Invalid(format!("unsupported cardstock '{}'", raw.details.stock))
        })?;
        let foil = raw.details.foil.trim() == "true";

        let details = Details {
            quantity,
            stock,
            foil,
        };
        let cardback_id = raw
            .cardback
            .as_deref()
            .map(|id| id.trim_matches(|c| c == ' ' || c == '"').to_owned())
            .filter(|id| !id.is_empty());
        if let Some(id) = &cardback_id {
            validate_drive_id(id)?;
        }
        let fronts = merge_cards(raw.fronts.cards, quantity)?;
        let backs = build_backs(raw.backs.cards, cardback_id.as_deref(), quantity)?;

        let order = Order {
            name: name.to_owned(),
            details,
            fronts,
            backs,
            cardback_id,
        };
        order.validate()?;
        Ok(order)
    }

    /// Validate an imported or edited order without granting access to local files.
    /// Legacy orders may omit backs; execution requires `validate_for_autofill`.
    pub fn validate(&self) -> Result<()> {
        validate_quantity(self.details.quantity)?;
        validate_field(&self.name, "order name")?;
        if self.details.stock == Cardstock::P10 && self.details.foil {
            return Err(Error::Invalid("plastic cardstock cannot be foil".into()));
        }
        if self.fronts.is_empty() {
            return Err(Error::Invalid("order has no front images".into()));
        }
        if let Some(id) = &self.cardback_id {
            validate_drive_id(id)?;
        }
        for (face, cards) in [("front", &self.fronts), ("back", &self.backs)] {
            if cards.len() > PROJECT_MAX_SIZE as usize {
                return Err(Error::Invalid(format!(
                    "order has too many {face} artwork records"
                )));
            }
            let mut covered = BTreeSet::new();
            for card in cards {
                validate_card(card)?;
                if card.slots.is_empty() || card.slots.len() > PROJECT_MAX_SIZE as usize {
                    return Err(Error::Invalid(format!(
                        "{face} artwork must have 1–{PROJECT_MAX_SIZE} slots"
                    )));
                }
                for &slot in &card.slots {
                    if slot >= self.details.quantity || !covered.insert(slot) {
                        return Err(Error::Invalid(format!(
                            "{face} slot {slot} is outside the order or assigned more than once"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn validate_for_autofill(&self) -> Result<()> {
        self.validate()?;
        for (face, cards) in [("front", &self.fronts), ("back", &self.backs)] {
            let covered: BTreeSet<_> = cards
                .iter()
                .flat_map(|card| card.slots.iter().copied())
                .collect();
            if covered.len() != self.details.quantity as usize {
                return Err(Error::Invalid(format!(
                    "choose artwork for every {face} slot before autofill"
                )));
            }
        }
        Ok(())
    }

    /// Every image in the order, fronts first, tagged with the face it belongs to.
    pub fn images(&self) -> impl Iterator<Item = (Face, &Card)> {
        let fronts = self.fronts.iter().map(|card| (Face::Front, card));
        let backs = self.backs.iter().map(|card| (Face::Back, card));
        fronts.chain(backs)
    }

    pub fn to_xml(&self) -> Result<String> {
        self.validate()?;
        let face = |cards: &[Card]| RawFace {
            cards: cards
                .iter()
                .map(|card| RawCard {
                    id: card.drive_id.clone(),
                    slots: card
                        .slots
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                    name: card.name.clone(),
                    query: card.query.clone(),
                    source_type: Some(
                        match card.source {
                            Source::GoogleDrive => "Google Drive",
                            Source::LocalFile => "Local File",
                        }
                        .into(),
                    ),
                })
                .collect(),
        };
        let raw = RawOrder {
            details: RawDetails {
                quantity: self.details.quantity.to_string(),
                stock: self.details.stock.label().into(),
                foil: self.details.foil.to_string(),
            },
            fronts: face(&self.fronts),
            backs: face(&self.backs),
            cardback: self.cardback_id.clone(),
        };
        let xml = quick_xml::se::to_string(&raw).map_err(|e| Error::Invalid(e.to_string()))?;
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{xml}\n"
        ))
    }
}

/// Merge cards that share a Drive id into one, unioning their slot assignments and
/// preserving first-seen order.
fn merge_cards(raw: Vec<RawCard>, quantity: u32) -> Result<Vec<Card>> {
    if raw.len() > PROJECT_MAX_SIZE as usize {
        return Err(Error::Invalid(
            "order has too many artwork records on one face".into(),
        ));
    }
    let mut cards: Vec<Card> = Vec::with_capacity(raw.len());
    for card in raw {
        let slots = parse_slots(&card.slots, quantity)?;
        let source = match card.source_type.as_deref() {
            Some("Local File") => Source::LocalFile,
            None | Some("Google Drive") => Source::GoogleDrive,
            Some(_) => {
                return Err(Error::Invalid(
                    "artwork has an unsupported source type".into(),
                ))
            }
        };
        let drive_id = card.id.trim_matches(|c| c == ' ' || c == '"').to_owned();
        let card = Card {
            drive_id,
            source,
            slots,
            name: card.name,
            query: card.query,
        };
        validate_card(&card)?;

        match cards
            .iter_mut()
            .find(|existing| existing.drive_id == card.drive_id && existing.source == card.source)
        {
            Some(existing) => existing.slots.extend(card.slots),
            None => cards.push(card),
        }
    }
    Ok(cards)
}

/// Build the back face, filling any slots the order leaves uncovered with the
/// common cardback image.
fn build_backs(raw: Vec<RawCard>, cardback: Option<&str>, quantity: u32) -> Result<Vec<Card>> {
    let mut backs = merge_cards(raw, quantity)?;

    let cardback_id = cardback.map(|id| id.trim_matches(|c| c == ' ' || c == '"'));
    if let Some(cardback_id) = cardback_id.filter(|id| !id.is_empty()) {
        let covered: BTreeSet<u32> = backs
            .iter()
            .flat_map(|card| card.slots.iter().copied())
            .collect();
        let missing: BTreeSet<u32> = (0..quantity)
            .filter(|slot| !covered.contains(slot))
            .collect();
        if !missing.is_empty() {
            if let Some(existing) = backs
                .iter_mut()
                .find(|card| card.drive_id == cardback_id && card.source == Source::GoogleDrive)
            {
                existing.slots.extend(missing);
            } else {
                backs.push(Card {
                    drive_id: cardback_id.to_owned(),
                    source: Source::GoogleDrive,
                    slots: missing,
                    name: None,
                    query: None,
                });
            }
        }
    }
    Ok(backs)
}

/// Parse slot strings such as `"0"`, `"1,2,3"`, or `"[2, 4, 5, 6]"` into a set.
fn parse_slots(text: &str, quantity: u32) -> Result<BTreeSet<u32>> {
    let text = text.trim();
    let text = if text.starts_with('[') && text.ends_with(']') {
        &text[1..text.len() - 1]
    } else {
        text
    };
    let mut slots = BTreeSet::new();
    for (index, piece) in text.split(',').enumerate() {
        if index >= PROJECT_MAX_SIZE as usize {
            return Err(Error::Invalid("artwork has too many slot values".into()));
        }
        let piece = piece.trim();
        if piece.is_empty() || !piece.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Error::Invalid(
                "artwork slots must be comma-separated nonnegative integers".into(),
            ));
        }
        let slot: u32 = piece
            .parse()
            .map_err(|_| Error::Invalid("artwork slot is too large".into()))?;
        if slot >= quantity {
            return Err(Error::Invalid(format!(
                "artwork slot {slot} is outside quantity {quantity}"
            )));
        }
        slots.insert(slot);
    }
    Ok(slots)
}

fn validate_quantity(quantity: u32) -> Result<()> {
    if !(1..=PROJECT_MAX_SIZE).contains(&quantity) {
        return Err(Error::Invalid(format!(
            "order quantity must be from 1 to {PROJECT_MAX_SIZE}"
        )));
    }
    Ok(())
}

fn validate_card(card: &Card) -> Result<()> {
    match card.source {
        Source::GoogleDrive => validate_drive_id(&card.drive_id)?,
        Source::LocalFile => {
            validate_field(&card.drive_id, "local artwork reference")?;
            if card.drive_id.is_empty() {
                return Err(Error::Invalid("local artwork reference is empty".into()));
            }
        }
    }
    if let Some(name) = &card.name {
        validate_field(name, "artwork name")?;
    }
    if let Some(query) = &card.query {
        validate_field(query, "artwork query")?;
    }
    Ok(())
}

// region raw XML shapes

#[derive(Deserialize, serde::Serialize)]
#[serde(rename = "order")]
struct RawOrder {
    details: RawDetails,
    #[serde(default)]
    fronts: RawFace,
    #[serde(default)]
    backs: RawFace,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cardback: Option<String>,
}

#[derive(Deserialize, serde::Serialize, Default)]
struct RawFace {
    #[serde(default, rename = "card")]
    cards: Vec<RawCard>,
}

#[derive(Deserialize, serde::Serialize)]
struct RawDetails {
    quantity: String,
    stock: String,
    #[serde(default)]
    foil: String,
}

#[derive(Deserialize, serde::Serialize)]
struct RawCard {
    id: String,
    #[serde(default)]
    slots: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    #[serde(
        default,
        rename = "sourceType",
        skip_serializing_if = "Option::is_none"
    )]
    source_type: Option<String>,
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        <order>
            <details>
                <quantity>3</quantity>
                <bracket>18</bracket>
                <stock>(S30) Standard Smooth</stock>
                <foil>false</foil>
            </details>
            <fronts>
                <card><id>front_a</id><slots>0</slots><name>Ace.png</name><query>ace</query></card>
                <card><id>front_b</id><slots>1</slots><name>King.jpg</name></card>
                <card><id>front_a</id><slots>2</slots><name>Ace.png</name></card>
            </fronts>
            <backs>
                <card><id>back_x</id><slots>0</slots><name>Special Back.png</name></card>
            </backs>
            <cardback>common_back</cardback>
        </order>
    "#;

    #[test]
    fn parses_details() {
        let order = Order::from_xml("sample", SAMPLE).unwrap();
        assert_eq!(order.details.quantity, 3);
        assert_eq!(order.details.stock, Cardstock::S30);
        assert!(!order.details.foil);
    }

    #[test]
    fn merges_cards_sharing_a_drive_id() {
        let order = Order::from_xml("sample", SAMPLE).unwrap();
        assert_eq!(order.fronts.len(), 2);
        let ace = order
            .fronts
            .iter()
            .find(|c| c.drive_id == "front_a")
            .unwrap();
        assert_eq!(ace.slots, BTreeSet::from([0, 2]));
    }

    #[test]
    fn fills_uncovered_back_slots_with_the_common_cardback() {
        let order = Order::from_xml("sample", SAMPLE).unwrap();
        let common = order
            .backs
            .iter()
            .find(|c| c.drive_id == "common_back")
            .unwrap();
        // slot 0 is taken by the explicit back; slots 1 and 2 fall back to the common cardback.
        assert_eq!(common.slots, BTreeSet::from([1, 2]));
    }

    #[test]
    fn parses_slot_range_syntax() {
        assert_eq!(
            parse_slots("[2, 4, 5, 6]", 7).unwrap(),
            BTreeSet::from([2, 4, 5, 6])
        );
        assert_eq!(parse_slots("0", 1).unwrap(), BTreeSet::from([0]));
        for invalid in [
            "",
            "[]",
            "0,",
            "0,,1",
            "a,1",
            "-1",
            "+1",
            "[0",
            "0]",
            "[[0]]",
            "4294967296",
            "7",
        ] {
            assert!(parse_slots(invalid, 7).is_err(), "{invalid:?}");
        }
        assert!(parse_slots(&vec!["0"; 613].join(","), 1).is_err());
    }

    #[test]
    fn rejects_oversized_orders() {
        for quantity in [0, 613, 700, u32::MAX] {
            let xml = SAMPLE
                .replace(
                    "<quantity>3</quantity>",
                    &format!("<quantity>{quantity}</quantity>"),
                )
                .replace("front_a", "invalid/id");
            // The quantity error must happen before constructing fallback backs
            // or examining card records (including this deliberately invalid ID).
            let error = Order::from_xml("big", &xml).unwrap_err().to_string();
            assert!(error.contains("order quantity"), "{quantity}: {error}");
        }
        let xml = SAMPLE.replace("<quantity>3</quantity>", "<quantity>612</quantity>");
        let order = Order::from_xml("maximum", &xml).unwrap();
        assert_eq!(
            order
                .backs
                .iter()
                .map(|card| card.slots.len())
                .sum::<usize>(),
            612
        );
        assert!(order.validate_for_autofill().is_err()); // fronts still cover only three slots
    }

    #[test]
    fn xml_round_trip_preserves_artwork_backs_and_escaped_names() {
        let mut order = Order::from_xml("sample", SAMPLE).unwrap();
        order.fronts[0].name = Some("Fire & Ice <alternate>.png".into());
        order.fronts[0].source = Source::LocalFile;
        order.fronts[0].drive_id = "/tmp/Fire & Ice.png".into();
        assert_eq!(
            Order::from_xml("sample", &order.to_xml().unwrap()).unwrap(),
            order
        );
    }

    #[test]
    fn explicit_common_back_and_fallback_slots_share_one_image() {
        let order = Order::from_xml(
            "sample",
            &SAMPLE.replace("<id>back_x</id>", "<id>common_back</id>"),
        )
        .unwrap();
        assert_eq!(order.backs.len(), 1);
        assert_eq!(order.backs[0].slots, BTreeSet::from([0, 1, 2]));
        assert_eq!(
            Order::from_xml("sample", &order.to_xml().unwrap()).unwrap(),
            order
        );
    }

    #[test]
    fn every_xml_drive_reference_rejects_paths_and_url_metacharacters() {
        for id in [
            "../escape",
            r"..\escape",
            "/tmp/escape",
            r"C:\escape",
            r"\\server\share",
            "art#/../../escape",
            "art?x=y",
            "art%2fescape",
            "é",
        ] {
            for original in ["front_a", "back_x", "common_back"] {
                assert!(
                    Order::from_xml("hostile", &SAMPLE.replace(original, id)).is_err(),
                    "{original}: {id}"
                );
            }
        }
        assert!(Order::from_xml("long ID", &SAMPLE.replace("front_a", &"a".repeat(257))).is_err());
    }

    #[test]
    fn malformed_slots_and_conflicting_assignments_do_not_silently_disappear() {
        for slots in ["bad", "-1", "0,broken", "3", "4294967295", "0,", ""] {
            let xml = SAMPLE.replacen("<slots>0</slots>", &format!("<slots>{slots}</slots>"), 1);
            assert!(Order::from_xml("bad slots", &xml).is_err(), "{slots:?}");
        }
        assert!(Order::from_xml(
            "overlap",
            &SAMPLE.replace("<slots>1</slots>", "<slots>0</slots>")
        )
        .is_err());
        let overlapping_backs = SAMPLE.replace(
            "</backs>",
            "<card><id>other_back</id><slots>0</slots></card></backs>",
        );
        assert!(Order::from_xml("overlap", &overlapping_backs).is_err());
    }

    #[test]
    fn source_is_validated_and_is_part_of_artwork_identity() {
        let xml = SAMPLE.replace(
            "<card><id>front_b</id>",
            "<card><id>front_a</id><sourceType>Local File</sourceType>",
        );
        let order = Order::from_xml("mixed sources", &xml).unwrap();
        assert_eq!(order.fronts.len(), 2);
        assert_eq!(order.fronts[0].source, Source::GoogleDrive);
        assert_eq!(order.fronts[0].slots, BTreeSet::from([0, 2]));
        assert_eq!(order.fronts[1].source, Source::LocalFile);
        assert_eq!(order.fronts[1].slots, BTreeSet::from([1]));
        assert_eq!(
            Order::from_xml("mixed sources", &order.to_xml().unwrap()).unwrap(),
            order
        );
        for source in ["", "HTTP", "google drive"] {
            let xml = SAMPLE.replacen(
                "<id>front_a</id>",
                &format!("<id>front_a</id><sourceType>{source}</sourceType>"),
                1,
            );
            assert!(Order::from_xml("unknown source", &xml).is_err());
        }
    }

    #[test]
    fn legacy_missing_backs_can_be_opened_but_cannot_run() {
        let xml = "<order><details><quantity>1</quantity><stock>(S30) Standard Smooth</stock></details><fronts><card><id>art</id><slots>0</slots></card></fronts></order>";
        let order = Order::from_xml("repair", xml).unwrap();
        assert!(order.validate().is_ok());
        assert!(order.validate_for_autofill().is_err());
        assert_eq!(
            Order::from_xml("repair", &order.to_xml().unwrap()).unwrap(),
            order
        );
    }

    #[test]
    fn hostile_xml_is_bounded_before_deserialization() {
        let attributes = (0..9)
            .map(|i| format!(" xmlns:n{i}=\"urn:{i}\""))
            .collect::<String>();
        let deep = format!(
            "<order>{}x{}</order>",
            "<nest>".repeat(17),
            "</nest>".repeat(17)
        );
        let records = format!(
            "<order>{}</order>",
            "<card/>".repeat(crate::validation::MAX_CARD_RECORDS + 1)
        );
        let many_on_one_face = SAMPLE.replace(
            "</fronts>",
            &format!(
                "{}</fronts>",
                "<card><id>front_a</id><slots>0</slots></card>".repeat(613)
            ),
        );
        let huge_field = SAMPLE.replace(
            "Ace.png",
            &"a".repeat(crate::validation::MAX_FIELD_BYTES + 1),
        );
        let chunked_field = SAMPLE.replace(
            "Ace.png",
            &format!("{}<![CDATA[{}]]>", "a".repeat(9000), "b".repeat(9000)),
        );
        let document_type =
            format!("<!DOCTYPE order [<!ENTITY secret SYSTEM 'file:///never-read'>]>{SAMPLE}");
        for xml in [
            SAMPLE.replace("<order>", &format!("<order{attributes}>")),
            deep,
            records,
            many_on_one_face,
            huge_field,
            chunked_field,
            document_type,
            format!("{SAMPLE}{SAMPLE}"),
            " ".repeat(MAX_XML_BYTES + 1),
        ] {
            assert!(Order::from_xml("hostile", &xml).is_err());
        }
        // Ordinary comments, BOM, CRLF and predefined escaped characters remain compatible.
        let xml = format!(
            "\u{feff}<!-- exported order -->\r\n{}",
            SAMPLE.replace("Ace.png", "Fire &amp; Ice.png")
        );
        assert!(Order::from_xml("normal", &xml).is_ok());
    }

    #[test]
    fn final_validation_checks_orders_constructed_without_the_parser() {
        let original = Order::from_xml("original", SAMPLE).unwrap();
        let mut edited = original.clone();
        edited.fronts[0].drive_id = "../escape".into();
        assert!(edited.validate().is_err());
        edited = original.clone();
        edited.fronts[0].slots.insert(3);
        assert!(edited.validate().is_err());
        assert_eq!(original, Order::from_xml("original", SAMPLE).unwrap());
        assert!(original.validate_for_autofill().is_ok());
    }

    #[test]
    fn required_values_and_duplicate_fields_cannot_be_hidden_by_xml_shape() {
        for replacement in [
            "<id/>",
            "<id>  </id>",
            "<id>front_a</id><id>other</id>",
            "<unrecognized>front_a</unrecognized>",
            "<id/><sourceType>Local File</sourceType>",
        ] {
            let xml = SAMPLE.replacen("<id>front_a</id>", replacement, 1);
            assert!(
                Order::from_xml("missing or duplicate ID", &xml).is_err(),
                "{replacement}"
            );
        }
        for replacement in [
            "<quantity>3</quantity><quantity>2</quantity>",
            "<quantity/>",
            "<unknown>3</unknown>",
        ] {
            assert!(Order::from_xml(
                "invalid quantity",
                &SAMPLE.replace("<quantity>3</quantity>", replacement)
            )
            .is_err());
        }
        assert!(Order::from_xml(
            "duplicate attribute",
            &SAMPLE.replace("<order>", "<order note=\"one\" note=\"two\">")
        )
        .is_err());
        // Extra legacy metadata such as <bracket> remains accepted, within limits.
        assert!(Order::from_xml("legacy", SAMPLE).is_ok());
    }
}
