//! The card-order data model and its XML parser.
//!
//! Mirrors the semantics of the reference Python tool's `order.py`: cards sharing
//! a Drive id within a face are merged, and any back slots left uncovered are
//! filled with the order's common cardback.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use crate::{Error, Result};

/// The largest project a MakePlayingCards family site accepts.
pub const PROJECT_MAX_SIZE: u32 = 612;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Face {
    Front,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
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
        let xml = tokio::fs::read_to_string(path).await?;
        Order::from_xml(&name, &xml)
    }

    pub fn from_xml(name: &str, xml: &str) -> Result<Order> {
        let raw: RawOrder = quick_xml::de::from_str(xml)?;

        let quantity: u32 = raw.details.quantity.trim().parse().map_err(|_| {
            Error::Invalid(format!(
                "quantity '{}' is not a number",
                raw.details.quantity
            ))
        })?;
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
        let fronts = merge_cards(raw.fronts.cards);
        let backs = build_backs(raw.backs.cards, cardback_id.as_deref(), quantity);

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

    pub(crate) fn validate(&self) -> Result<()> {
        if self.details.quantity == 0 {
            return Err(Error::Invalid("order has no cards".into()));
        }
        if self.details.quantity > PROJECT_MAX_SIZE {
            return Err(Error::Invalid(format!(
                "order quantity {} exceeds the maximum project size of {PROJECT_MAX_SIZE}",
                self.details.quantity
            )));
        }
        if self.details.stock == Cardstock::P10 && self.details.foil {
            return Err(Error::Invalid("plastic cardstock cannot be foil".into()));
        }
        if self.fronts.is_empty() {
            return Err(Error::Invalid("order has no front images".into()));
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
fn merge_cards(raw: Vec<RawCard>) -> Vec<Card> {
    let mut cards: Vec<Card> = Vec::with_capacity(raw.len());
    for card in raw {
        let slots = parse_slots(&card.slots);
        let source = match card.source_type.as_deref() {
            Some("Local File") => Source::LocalFile,
            _ => Source::GoogleDrive,
        };
        let drive_id = card.id.trim_matches(|c| c == ' ' || c == '"').to_owned();

        match cards
            .iter_mut()
            .find(|existing| existing.drive_id == drive_id)
        {
            Some(existing) => existing.slots.extend(slots),
            None => cards.push(Card {
                drive_id,
                source,
                slots,
                name: card.name,
                query: card.query,
            }),
        }
    }
    cards
}

/// Build the back face, filling any slots the order leaves uncovered with the
/// common cardback image.
fn build_backs(raw: Vec<RawCard>, cardback: Option<&str>, quantity: u32) -> Vec<Card> {
    let mut backs = merge_cards(raw);

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
            if let Some(existing) = backs.iter_mut().find(|card| card.drive_id == cardback_id) {
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
    backs
}

/// Parse slot strings such as `"0"`, `"1,2,3"`, or `"[2, 4, 5, 6]"` into a set.
fn parse_slots(text: &str) -> BTreeSet<u32> {
    text.trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .filter_map(|piece| piece.trim().parse().ok())
        .collect()
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
        assert_eq!(parse_slots("[2, 4, 5, 6]"), BTreeSet::from([2, 4, 5, 6]));
        assert_eq!(parse_slots("0"), BTreeSet::from([0]));
        assert_eq!(parse_slots(""), BTreeSet::new());
    }

    #[test]
    fn rejects_oversized_orders() {
        let xml = SAMPLE.replace("<quantity>3</quantity>", "<quantity>700</quantity>");
        assert!(matches!(
            Order::from_xml("big", &xml),
            Err(Error::Invalid(_))
        ));
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
}
