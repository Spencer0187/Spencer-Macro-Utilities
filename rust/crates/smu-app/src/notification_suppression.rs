use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

pub const ADMIN_ELEVATION_WARNING_ID: &str = "admin_elevation_warning";
const SUPPRESSED_NOTIFICATIONS_KEY: &str = "suppressed_notifications";
const LEGACY_ADMIN_WARNING_KEY: &str = "DontShowAdminWarning";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NotificationSuppressionState {
    ids: BTreeSet<String>,
}

impl NotificationSuppressionState {
    pub fn apply_metadata(&mut self, metadata: &Map<String, Value>) {
        let mut ids = BTreeSet::new();
        if let Some(values) = metadata.get(SUPPRESSED_NOTIFICATIONS_KEY).and_then(Value::as_array) {
            ids.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
        }
        if metadata.get(LEGACY_ADMIN_WARNING_KEY).and_then(Value::as_bool) == Some(true) {
            ids.insert(ADMIN_ELEVATION_WARNING_ID.to_owned());
        }
        self.ids = ids;
    }

    #[must_use]
    pub fn metadata_overlay(&self) -> Map<String, Value> {
        let suppressed = self.ids.iter().cloned().map(Value::String).collect::<Vec<_>>();
        Map::from_iter([
            (SUPPRESSED_NOTIFICATIONS_KEY.to_owned(), Value::Array(suppressed)),
            (
                LEGACY_ADMIN_WARNING_KEY.to_owned(),
                json!(self.is_suppressed(ADMIN_ELEVATION_WARNING_ID)),
            ),
        ])
    }

    #[must_use]
    pub fn is_suppressed(&self, id: &str) -> bool {
        !id.is_empty() && self.ids.contains(id)
    }

    pub fn set_suppressed(&mut self, id: &str, suppressed: bool) -> bool {
        if id.is_empty() {
            return false;
        }
        if suppressed { self.ids.insert(id.to_owned()) } else { self.ids.remove(id) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_load_matches_cpp_array_and_legacy_merge() {
        let mut state = NotificationSuppressionState::default();
        state.apply_metadata(&Map::from_iter([
            (SUPPRESSED_NOTIFICATIONS_KEY.to_owned(), json!(["zeta", 7, "alpha", "zeta"])),
            (LEGACY_ADMIN_WARNING_KEY.to_owned(), json!(true)),
        ]));

        assert!(state.is_suppressed("alpha"));
        assert!(state.is_suppressed("zeta"));
        assert!(state.is_suppressed(ADMIN_ELEVATION_WARNING_ID));
        assert_eq!(
            state.metadata_overlay()[SUPPRESSED_NOTIFICATIONS_KEY],
            json!(["admin_elevation_warning", "alpha", "zeta"])
        );
        assert_eq!(state.metadata_overlay()[LEGACY_ADMIN_WARNING_KEY], json!(true));
    }

    #[test]
    fn apply_replaces_previous_state_and_false_legacy_flag_does_not_remove_array_entry() {
        let mut state = NotificationSuppressionState::default();
        assert!(state.set_suppressed("old", true));
        state.apply_metadata(&Map::from_iter([
            (SUPPRESSED_NOTIFICATIONS_KEY.to_owned(), json!([ADMIN_ELEVATION_WARNING_ID])),
            (LEGACY_ADMIN_WARNING_KEY.to_owned(), json!(false)),
        ]));

        assert!(!state.is_suppressed("old"));
        assert!(state.is_suppressed(ADMIN_ELEVATION_WARNING_ID));
    }

    #[test]
    fn missing_or_malformed_metadata_clears_state_like_cpp_loader() {
        let mut state = NotificationSuppressionState::default();
        assert!(state.set_suppressed("old", true));
        state.apply_metadata(&Map::from_iter([(
            SUPPRESSED_NOTIFICATIONS_KEY.to_owned(),
            json!("not-an-array"),
        )]));
        assert_eq!(state, NotificationSuppressionState::default());
    }

    #[test]
    fn empty_ids_are_ignored() {
        let mut state = NotificationSuppressionState::default();
        assert!(!state.set_suppressed("", true));
        assert!(!state.is_suppressed(""));
    }
}
