use std::collections::BTreeSet;

use serde::Deserialize;

use crate::decklist::{ArtworkSelection, DecklistPreview};
use crate::{Card, Cardstock, Details, Error, Order, Result, Source};

#[derive(Debug, Clone, Deserialize)]
pub struct CardbackSelection {
    pub drive_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewOrderOptions {
    pub name: String,
    pub stock: Cardstock,
    pub foil: bool,
    pub cardback: Option<CardbackSelection>,
}

impl NewOrderOptions {
    /// Validate all draft changes before the session replaces its loaded order.
    pub fn revise(
        &self,
        current: &Order,
        text: &str,
        selections: &[ArtworkSelection],
    ) -> Result<Order> {
        let template = self.template()?;
        let mut base = current.clone();
        base.name = template.name;
        base.details.stock = template.details.stock;
        base.details.foil = template.details.foil;
        if base.cardback_id.is_none() {
            base.backs.extend(template.backs);
            base.cardback_id = template.cardback_id;
        }
        // Resolve text against the original names first: mixed-back decklists
        // can explicitly name the old common back on some lines.
        let mut edited = base.edit_decklist(text, selections)?;
        if let Some(back) = &self.cardback {
            if edited.cardback_id.as_deref() != Some(&back.drive_id) {
                if let Some(old_id) = edited
                    .cardback_id
                    .as_deref()
                    .filter(|id| edited.backs.iter().any(|card| card.drive_id == *id))
                {
                    edited = edited.change_artwork(
                        old_id,
                        &ArtworkSelection {
                            query: String::new(),
                            face: "back".into(),
                            drive_id: back.drive_id.clone(),
                            name: back.name.clone(),
                        },
                    )?;
                } else {
                    edited.cardback_id = Some(back.drive_id.clone());
                }
            }
        }
        Ok(edited)
    }

    fn template(&self) -> Result<Order> {
        let name = self.name.trim();
        if name.is_empty() || name.len() > 200 {
            return Err(Error::Invalid(
                "enter an order name of 1–200 characters".into(),
            ));
        }
        if self.stock == Cardstock::P10 && self.foil {
            return Err(Error::Invalid("plastic cardstock cannot be foil".into()));
        }
        let backs = if let Some(back) = &self.cardback {
            if back.drive_id.is_empty()
                || !back
                    .drive_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(Error::Invalid(
                    "selected cardback has an invalid Google Drive ID".into(),
                ));
            }
            vec![Card {
                drive_id: back.drive_id.clone(),
                name: Some(back.name.clone()),
                source: Source::GoogleDrive,
                slots: BTreeSet::new(),
                query: None,
            }]
        } else {
            Vec::new()
        };
        Ok(Order {
            name: name.into(),
            details: Details {
                quantity: 0,
                stock: self.stock,
                foil: self.foil,
            },
            fronts: Vec::new(),
            backs,
            cardback_id: self.cardback.as_ref().map(|back| back.drive_id.clone()),
        })
    }

    pub fn preview(&self, text: &str, selections: &[ArtworkSelection]) -> Result<DecklistPreview> {
        self.template()?.preview_decklist(text, selections)
    }

    pub fn create(&self, text: &str, selections: &[ArtworkSelection]) -> Result<Order> {
        let order = self.template()?.edit_decklist(text, selections)?;
        let covered: BTreeSet<_> = order
            .backs
            .iter()
            .flat_map(|back| back.slots.iter().copied())
            .collect();
        if covered != (0..order.details.quantity).collect() {
            return Err(Error::Invalid(
                "choose a common cardback for cards without a custom reverse".into(),
            ));
        }
        Ok(order)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> NewOrderOptions {
        NewOrderOptions {
            name: "New deck".into(),
            stock: Cardstock::S30,
            foil: false,
            cardback: None,
        }
    }
    fn art(query: &str, face: &str, id: &str) -> ArtworkSelection {
        ArtworkSelection {
            query: query.into(),
            face: face.into(),
            drive_id: id.into(),
            name: format!("{query}.png"),
        }
    }
    #[test]
    fn text_import_needs_no_existing_order_and_reports_missing_artwork() {
        let preview = options()
            .preview("2 Island\n1 Lightning Bolt", &[])
            .unwrap();
        assert_eq!(preview.quantity, 3);
        assert_eq!(preview.unresolved.len(), 2);
        assert!(preview.needs_common_back);
    }

    #[test]
    fn revisions_validate_settings_and_text_atomically_and_preserve_artwork() {
        let mut opts = options();
        opts.cardback = Some(CardbackSelection {
            drive_id: "common".into(),
            name: "Common.png".into(),
        });
        let original = opts
            .create(
                "2 Island\n1 Delver | Insect",
                &[
                    art("Island", "front", "island"),
                    art("Delver", "front", "delver"),
                    art("Insect", "back", "insect"),
                ],
            )
            .unwrap();
        let text = original.to_decklist().unwrap();
        assert_eq!(opts.revise(&original, &text, &[]).unwrap(), original);
        opts.name = "Revised deck".into();
        opts.stock = Cardstock::M31;
        opts.foil = true;
        let revised = opts.revise(&original, &text, &[]).unwrap();
        assert_eq!(revised.name, "Revised deck");
        assert_eq!(revised.details.stock, Cardstock::M31);
        assert!(revised.details.foil);
        assert_eq!(revised.fronts, original.fronts);
        assert_eq!(revised.backs, original.backs);
        let snapshot = original.clone();
        assert!(opts.revise(&original, "0 Island", &[]).is_err());
        opts.stock = Cardstock::P10;
        assert!(opts.revise(&original, &text, &[]).is_err());
        assert_eq!(original, snapshot);
        opts.foil = false;
        opts.cardback = Some(CardbackSelection {
            drive_id: "new-common".into(),
            name: "New.png".into(),
        });
        let revised = opts.revise(&original, &text, &[]).unwrap();
        assert_eq!(
            revised
                .backs
                .iter()
                .find(|card| card.drive_id == "insect")
                .unwrap()
                .slots,
            BTreeSet::from([2])
        );
        assert_eq!(
            revised
                .backs
                .iter()
                .find(|card| card.drive_id == "new-common")
                .unwrap()
                .slots,
            BTreeSet::from([0, 1])
        );
    }

    #[test]
    fn changing_common_back_resolves_mixed_back_text_using_the_original_names() {
        let mut opts = options();
        opts.cardback = Some(CardbackSelection {
            drive_id: "common".into(),
            name: "Common.png".into(),
        });
        let original = opts
            .create(
                "1 Island | Insect\n1 Island",
                &[
                    art("Island", "front", "island"),
                    art("Insect", "back", "insect"),
                ],
            )
            .unwrap();
        let text = original.to_decklist().unwrap();
        assert!(text.contains("Island | Common"));
        opts.cardback = Some(CardbackSelection {
            drive_id: "replacement".into(),
            name: "New back.png".into(),
        });
        let edited = opts.revise(&original, &text, &[]).unwrap();
        assert_eq!(edited.fronts, original.fronts);
        assert_eq!(
            edited
                .backs
                .iter()
                .find(|card| card.drive_id == "insect")
                .unwrap()
                .slots,
            BTreeSet::from([0])
        );
        assert_eq!(
            edited
                .backs
                .iter()
                .find(|card| card.drive_id == "replacement")
                .unwrap()
                .slots,
            BTreeSet::from([1])
        );
    }
    #[test]
    fn new_order_requires_artwork_and_back_before_creation() {
        assert!(options().create("2 Island", &[]).is_err());
        let selections = [art("Island", "front", "island")];
        assert!(options().create("2 Island", &selections).is_err());
        let mut opts = options();
        opts.cardback = Some(CardbackSelection {
            drive_id: "back".into(),
            name: "Custom back.png".into(),
        });
        let order = opts.create("2 Island", &selections).unwrap();
        assert_eq!(order.backs[0].slots, BTreeSet::from([0, 1]));
        assert_eq!(order.cardback_id.as_deref(), Some("back"));
        assert_eq!(
            Order::from_xml("New deck", &order.to_xml().unwrap()).unwrap(),
            order
        );
    }
    #[test]
    fn all_double_faced_decks_need_no_common_back() {
        let opts = options();
        assert!(
            !opts
                .preview("1 Delver | Insect", &[])
                .unwrap()
                .needs_common_back
        );
        let selections = [
            art("Delver", "front", "delver"),
            art("Insect", "back", "insect"),
        ];
        let order = opts.create("1 Delver | Insect", &selections).unwrap();
        assert_eq!(order.backs[0].drive_id, "insect");
    }
    #[test]
    fn selected_common_back_does_not_replace_custom_backs() {
        let mut opts = options();
        opts.stock = Cardstock::M31;
        opts.foil = true;
        opts.cardback = Some(CardbackSelection {
            drive_id: "common".into(),
            name: "Common.png".into(),
        });
        let selections = [
            art("Delver", "front", "delver"),
            art("Insect", "back", "insect"),
            art("Island", "front", "island"),
        ];
        let order = opts
            .create("1 Delver | Insect\n2 Island", &selections)
            .unwrap();
        assert_eq!(order.backs[0].slots, BTreeSet::from([0]));
        assert_eq!(order.backs[1].slots, BTreeSet::from([1, 2]));
        assert_eq!(order.details.stock, Cardstock::M31);
        assert!(order.details.foil);
    }
    #[test]
    fn invalid_settings_and_oversized_decks_are_rejected() {
        assert!(!options()
            .preview("613 Island", &[])
            .unwrap()
            .issues
            .is_empty());
        let mut opts = options();
        opts.name = " ".into();
        assert!(opts.preview("1 Island", &[]).is_err());
        opts.name = "Deck".into();
        opts.stock = Cardstock::P10;
        opts.foil = true;
        assert!(opts.preview("1 Island", &[]).is_err());
    }
}
