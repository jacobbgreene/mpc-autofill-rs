//! Replace one image without rebuilding the deck or changing slot pairings.
use crate::decklist::ArtworkSelection;
use crate::{Error, Face, Order, Result, Source};

impl Order {
    pub fn change_artwork(&self, drive_id: &str, selection: &ArtworkSelection) -> Result<Self> {
        let face = match selection.face.as_str() {
            "front" => Face::Front,
            "back" => Face::Back,
            _ => return Err(Error::Invalid("choose a front or back image".into())),
        };
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
        let mut edited = self.clone();
        let cards = match face {
            Face::Front => &mut edited.fronts,
            Face::Back => &mut edited.backs,
        };
        let index = cards
            .iter()
            .position(|card| card.drive_id == drive_id)
            .ok_or_else(|| Error::Invalid("this image is no longer in the order".into()))?;
        if cards[index].drive_id == selection.drive_id && cards[index].source == Source::GoogleDrive
        {
            return Ok(edited);
        }
        let mut replacement = cards.remove(index);
        replacement.drive_id = selection.drive_id.clone();
        replacement.name = Some(selection.name.clone());
        replacement.source = Source::GoogleDrive;
        if !selection.query.trim().is_empty() {
            replacement.query = Some(selection.query.trim().into());
        }
        if let Some(existing) = cards
            .iter_mut()
            .find(|card| card.drive_id == replacement.drive_id)
        {
            existing.slots.extend(replacement.slots);
            existing.source = Source::GoogleDrive;
        } else {
            cards.insert(index, replacement);
        }
        if face == Face::Back && self.cardback_id.as_deref() == Some(drive_id) {
            edited.cardback_id = Some(selection.drive_id.clone());
        }
        edited.validate()?;
        Ok(edited)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn order() -> Order {
        Order::from_xml("Deck", r#"<order><details><quantity>3</quantity><stock>(S33) Superior Smooth</stock><foil>true</foil></details><fronts><card><id>shared</id><name>Island.png</name><query>Island</query><slots>0,2</slots></card><card><id>delver</id><name>Delver.png</name><slots>1</slots></card></fronts><backs><card><id>insect</id><name>Insect.png</name><slots>1,2</slots></card></backs><cardback>shared</cardback></order>"#).unwrap()
    }
    fn art(face: &str, id: &str) -> ArtworkSelection {
        ArtworkSelection {
            face: face.into(),
            query: "Island".into(),
            drive_id: id.into(),
            name: "Island (Alternate).png".into(),
        }
    }

    #[test]
    fn changing_front_art_preserves_copies_mixed_backs_and_print_settings() {
        let original = order();
        let edited = original
            .change_artwork("shared", &art("front", "new-front"))
            .unwrap();
        assert_eq!(edited.fronts[0].drive_id, "new-front");
        assert_eq!(edited.fronts[0].slots, BTreeSet::from([0, 2]));
        assert_eq!(edited.fronts[0].query.as_deref(), Some("Island"));
        assert_eq!(edited.fronts[1], original.fronts[1]);
        assert_eq!(edited.backs, original.backs);
        assert_eq!(edited.cardback_id, original.cardback_id);
        assert_eq!(edited.details, original.details);
        assert_eq!(
            Order::from_xml("Deck", &edited.to_xml().unwrap()).unwrap(),
            edited
        );
        assert_eq!(original, order());
    }

    #[test]
    fn changing_common_back_preserves_custom_reverse_and_front_with_same_id() {
        let original = order();
        let edited = original
            .change_artwork("shared", &art("back", "new-back"))
            .unwrap();
        assert_eq!(edited.cardback_id.as_deref(), Some("new-back"));
        assert_eq!(edited.fronts, original.fronts);
        assert_eq!(edited.backs[0], original.backs[0]);
        assert_eq!(edited.backs[1].drive_id, "new-back");
        assert_eq!(edited.backs[1].slots, BTreeSet::from([0]));
        assert_eq!(
            Order::from_xml("Deck", &edited.to_xml().unwrap()).unwrap(),
            edited
        );
    }

    #[test]
    fn selecting_art_already_used_on_same_face_merges_slots_without_duplicates() {
        let original = order();
        let edited = original
            .change_artwork("delver", &art("front", "shared"))
            .unwrap();
        assert_eq!(edited.fronts.len(), 1);
        assert_eq!(edited.fronts[0].slots, BTreeSet::from([0, 1, 2]));
        assert_eq!(edited.backs, original.backs);
        let edited = original
            .change_artwork("shared", &art("back", "insect"))
            .unwrap();
        assert_eq!(edited.backs.len(), 1);
        assert_eq!(edited.backs[0].slots, BTreeSet::from([0, 1, 2]));
        assert_eq!(edited.cardback_id.as_deref(), Some("insect"));
        assert_eq!(
            Order::from_xml("Deck", &edited.to_xml().unwrap()).unwrap(),
            edited
        );
    }

    #[test]
    fn same_art_is_a_noop_and_invalid_or_stale_edits_never_mutate_order() {
        let original = order();
        assert_eq!(
            original
                .change_artwork("shared", &art("front", "shared"))
                .unwrap(),
            original
        );
        assert!(original
            .change_artwork("missing", &art("front", "new"))
            .is_err());
        assert!(original
            .change_artwork("insect", &art("front", "new"))
            .is_err());
        assert!(original
            .change_artwork("shared", &art("invalid", "new"))
            .is_err());
        assert!(original
            .change_artwork("shared", &art("front", "../invalid"))
            .is_err());
        assert_eq!(original, order());
    }
}
