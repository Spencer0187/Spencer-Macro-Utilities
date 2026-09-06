use crate::profile_adapter::LiveSavedSettings;
use serde_json::Value;
use smu_profile::payload::PreservedProfilePayload;
use smu_scripting::ScriptManager;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ScriptProfileBridge {
    manager: Arc<ScriptManager>,
    saved_settings: Arc<LiveSavedSettings>,
}

impl ScriptProfileBridge {
    #[must_use]
    pub const fn new(manager: Arc<ScriptManager>, saved_settings: Arc<LiveSavedSettings>) -> Self {
        Self { manager, saved_settings }
    }

    #[must_use]
    pub fn serialize_scripts(&self) -> Value {
        self.manager.serialize()
    }

    pub fn apply_profile(&self, payload: &PreservedProfilePayload) {
        self.saved_settings.replace(payload);
        let scripts = payload
            .raw_value("imported_scripts")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        self.manager.deserialize(&scripts);
    }

    pub fn update_saved_settings(&self, payload: &PreservedProfilePayload) {
        self.saved_settings.replace(payload);
    }

    pub fn shutdown(&self) {
        self.manager.shutdown();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::ScriptProfileBridge;
    use crate::profile_adapter::LiveSavedSettings;
    use serde_json::json;
    use smu_profile::SavedSettingSource;
    use smu_profile::payload::PreservedProfilePayload;
    use smu_scripting::{ScriptManager, ScriptManagerConfig, ScriptServices};
    use std::sync::Arc;

    fn bridge() -> ScriptProfileBridge {
        let manager = Arc::new(ScriptManager::new(
            ScriptManagerConfig::new("test"),
            ScriptServices::default(),
        ));
        ScriptProfileBridge::new(manager, Arc::new(LiveSavedSettings::default()))
    }

    #[test]
    fn profile_bridge_updates_saved_settings_and_defaults_missing_scripts_to_empty() {
        let bridge = bridge();
        let payload = PreservedProfilePayload::from_value(&json!({
            "ProfileName": "Bridge",
            "clip_delay": 77
        }))
        .expect("payload");

        bridge.apply_profile(&payload);

        assert_eq!(bridge.serialize_scripts(), json!([]));
        assert!(bridge.saved_settings.get_saved_setting("clip_delay").is_some());
    }

    #[test]
    fn profile_bridge_round_trips_imported_script_records() {
        let bridge = bridge();
        let scripts = json!([{
            "path": "/tmp/missing.lua",
            "hotkey": 0,
            "enabled": true,
            "disableOutsideRoblox": false,
            "ui": {}
        }]);
        let payload = PreservedProfilePayload::from_value(&json!({
            "ProfileName": "Bridge",
            "imported_scripts": scripts
        }))
        .expect("payload");

        bridge.apply_profile(&payload);
        let serialized = bridge.serialize_scripts();

        assert_eq!(serialized.as_array().map(Vec::len), Some(1));
        assert_eq!(serialized[0]["path"], "/tmp/missing.lua");
        bridge.shutdown();
    }
}
