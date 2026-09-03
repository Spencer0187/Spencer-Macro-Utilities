//! Differentially frozen profile-payload validation and application semantics.
//!
//! The C++ implementation mixes persistence with process-lifetime globals. During the rewrite,
//! this compatibility state models exactly what the current serializer can observe after a load.
//! Its schema and initial snapshot are generated from the production C++ maps by
//! `tools/update_compat_fixtures.py profiles`.

use serde::Deserialize;
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const PAYLOAD_SPEC: &str = include_str!("../../../../compat/profiles/payload_spec.json");
#[cfg(test)]
const PAYLOAD_FIXTURE: &str = include_str!("../../../../compat/profiles/payload_cases.json");
const DEFAULT_INSTANCE_BUFFER_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum NumericKind {
    Int,
    Float,
    Unsigned,
}

#[derive(Debug, Deserialize)]
struct PayloadSchema {
    bool_keys: BTreeSet<String>,
    numeric_kinds: BTreeMap<String, NumericKind>,
    char_buffers: BTreeMap<String, usize>,
    section_amounts: usize,
}

#[derive(Debug, Deserialize)]
struct PayloadSpecification {
    schema: PayloadSchema,
    default_payload: Map<String, Value>,
}

fn specification() -> Option<&'static PayloadSpecification> {
    static SPECIFICATION: OnceLock<Option<PayloadSpecification>> = OnceLock::new();
    SPECIFICATION.get_or_init(|| serde_json::from_str(PAYLOAD_SPEC).ok()).as_ref()
}

/// In-memory profile state with the same serialized surface as the current C++ implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilePayloadState {
    values: Map<String, Value>,
}

impl Default for ProfilePayloadState {
    fn default() -> Self {
        Self {
            values: specification()
                .map(|specification| specification.default_payload.clone())
                .unwrap_or_default(),
        }
    }
}

impl ProfilePayloadState {
    /// Returns a normalized snapshot equivalent to `SerializeProfileData()` in the payload oracle.
    #[must_use]
    pub fn snapshot(&self) -> Value {
        Value::Object(self.values.clone())
    }

    /// Applies one partial payload with the current C++ compatibility semantics.
    ///
    /// Invalid payloads are rejected before mutation, matching the validator gate at the start of
    /// `ApplyProfileData`.
    #[must_use]
    pub fn apply(&mut self, payload: &Value) -> bool {
        if !validate(payload) {
            return false;
        }
        let Some(settings) = payload.as_object() else {
            return false;
        };
        let Some(specification) = specification() else {
            return false;
        };
        let schema = &specification.schema;

        // Loading any profile rebuilds all multi-instance vectors and resets the selected wallhop.
        self.values.remove("extra_wallhop_instances");
        self.values.remove("extra_presskey_instances");
        self.values.remove("extra_spamkey_instances");
        self.values.insert("selected_wallhop_instance".to_owned(), Value::Number(Number::from(0)));

        for key in &schema.bool_keys {
            if let Some(value) = settings.get(key) {
                self.values.insert(key.clone(), value.clone());
            }
        }

        for (key, kind) in &schema.numeric_kinds {
            let Some(value) = settings.get(key) else {
                continue;
            };
            if let Some(normalized) = normalize_numeric(value, *kind) {
                self.values.insert(key.clone(), normalized);
            } else {
                return false;
            }
        }

        for (key, capacity) in &schema.char_buffers {
            let Some(value) = settings.get(key) else {
                continue;
            };
            let Some(text) = value.as_str() else {
                return false;
            };
            self.values.insert(key.clone(), Value::String(truncate_utf8_buffer(text, *capacity)));
        }

        self.sync_observable_runtime_from_buffers(settings);
        self.apply_prefix_bool_array(settings, "section_toggles");
        self.apply_disable_outside(settings);
        self.apply_section_order(settings);

        if let Some(value) = settings.get("text") {
            self.values.insert("text".to_owned(), value.clone());
        }
        for key in ["screen_width", "screen_height"] {
            if let Some(value) = settings.get(key) {
                let Some(normalized) = normalize_numeric(value, NumericKind::Int) else {
                    return false;
                };
                self.values.insert(key.to_owned(), normalized);
            }
        }

        self.apply_extra_instances(settings);
        self.clamp_selected_wallhop();

        // The deterministic payload oracle intentionally stubs ScriptManager serialization to [].
        self.values.insert("imported_scripts".to_owned(), Value::Array(Vec::new()));
        true
    }

    fn sync_observable_runtime_from_buffers(&mut self, settings: &Map<String, Value>) {
        const DIRECT_INTEGER_SYNCS: &[(&str, &str)] = &[
            ("ItemDesyncSlot", "desync_slot"),
            ("ItemSpeedSlot", "speed_slot"),
            ("ItemClipSlot", "clip_slot"),
            ("ItemClipDelay", "clip_delay"),
            ("RobloxWallWalkValueDelayChar", "RobloxWallWalkValueDelay"),
            ("RobloxPixelValueChar", "RobloxPixelValue"),
            ("AntiAFKTimeChar", "AntiAFKTime"),
            ("AutoHHJKey1TimeChar", "AutoHHJKey1Time"),
            ("AutoHHJKey2TimeChar", "AutoHHJKey2Time"),
        ];

        for (buffer_key, numeric_key) in DIRECT_INTEGER_SYNCS {
            let Some(parsed) = self.present_buffer_i32(settings, buffer_key) else {
                continue;
            };
            self.set_i32_if_serialized(numeric_key, parsed);
        }

        if let Some(parsed) = self.present_buffer_i32(settings, "WallhopPixels")
            && parsed != i32::MIN
        {
            self.set_i32_if_serialized("wallhop_dx", parsed);
            self.set_i32_if_serialized("wallhop_dy", -parsed);
        }
        if let Some(parsed) = self.present_buffer_i32(settings, "WallhopVerticalChar") {
            self.set_i32_if_serialized("wallhop_vertical", parsed);
        }
        if let Some(parsed) = self.present_buffer_i32(settings, "RobloxPixelValueChar")
            && parsed != i32::MIN
        {
            self.set_i32_if_serialized("speed_strengthx", parsed);
            self.set_i32_if_serialized("speed_strengthy", -parsed);
        }

        if settings.get("SpamDelay").is_some_and(Value::is_string)
            && let Some(buffer) = self.values.get("SpamDelay").and_then(Value::as_str)
            && let Some(parsed) = parse_cpp_f32(buffer)
            && let Some(real_delay) = cpp_spam_real_delay(parsed)
        {
            self.values.insert(
                "spam_delay".to_owned(),
                float_value(f64::from(parsed)).unwrap_or(Value::Null),
            );
            self.set_i32_if_serialized("real_delay", real_delay);
        }
    }

    fn present_buffer_i32(&self, settings: &Map<String, Value>, key: &str) -> Option<i32> {
        if !settings.get(key).is_some_and(Value::is_string) {
            return None;
        }
        self.values.get(key).and_then(Value::as_str).and_then(parse_cpp_i32)
    }

    fn set_i32_if_serialized(&mut self, key: &str, value: i32) {
        if self.values.contains_key(key) {
            self.values.insert(key.to_owned(), Value::Number(Number::from(value)));
        }
    }

    fn apply_prefix_bool_array(&mut self, settings: &Map<String, Value>, key: &str) {
        let Some(incoming) = settings.get(key).and_then(Value::as_array) else {
            return;
        };
        let Some(existing) = self.values.get_mut(key).and_then(Value::as_array_mut) else {
            return;
        };
        for (destination, source) in existing.iter_mut().zip(incoming) {
            *destination = source.clone();
        }
    }

    fn apply_disable_outside(&mut self, settings: &Map<String, Value>) {
        let incoming = settings.get("disable_outside_roblox").and_then(Value::as_array);
        if let Some(incoming) = incoming {
            if let Some(existing) =
                self.values.get_mut("disable_outside_roblox").and_then(Value::as_array_mut)
            {
                for (destination, source) in existing.iter_mut().zip(incoming) {
                    *destination = source.clone();
                }
            }
        } else {
            let freeze = self.bool_value("freezeoutsideroblox").unwrap_or(false);
            let unequip = self.bool_value("unequipinroblox").unwrap_or(false);
            let press = self.bool_value("presskeyinroblox").unwrap_or(false);
            self.set_disable_index(0, !freeze);
            self.set_disable_index(4, unequip);
            self.set_disable_index(5, press);
        }

        if let Some(disable) = self.values.get("disable_outside_roblox").and_then(Value::as_array) {
            let freeze = !disable.first().and_then(Value::as_bool).unwrap_or(false);
            let unequip = disable.get(4).and_then(Value::as_bool).unwrap_or(false);
            let press = disable.get(5).and_then(Value::as_bool).unwrap_or(false);
            self.values.insert("freezeoutsideroblox".to_owned(), Value::Bool(freeze));
            self.values.insert("unequipinroblox".to_owned(), Value::Bool(unequip));
            self.values.insert("presskeyinroblox".to_owned(), Value::Bool(press));
        }
    }

    fn bool_value(&self, key: &str) -> Option<bool> {
        self.values.get(key).and_then(Value::as_bool)
    }

    fn set_disable_index(&mut self, index: usize, value: bool) {
        if let Some(disable) =
            self.values.get_mut("disable_outside_roblox").and_then(Value::as_array_mut)
            && let Some(slot) = disable.get_mut(index)
        {
            *slot = Value::Bool(value);
        }
    }

    fn disable_index(&self, index: usize) -> bool {
        self.values
            .get("disable_outside_roblox")
            .and_then(Value::as_array)
            .and_then(|values| values.get(index))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    fn apply_section_order(&mut self, settings: &Map<String, Value>) {
        let Some(incoming) = settings.get("section_order_vector").and_then(Value::as_array) else {
            return;
        };
        let Some(section_count) =
            specification().map(|specification| specification.schema.section_amounts)
        else {
            return;
        };
        let mut order: Vec<i32> = incoming.iter().filter_map(normalize_i32).collect();

        if !order.contains(&13) && order.len() >= 7 {
            order.insert(7, 13);
        }
        if !order.contains(&15) && order.len() >= 5 {
            order.insert(4, 15);
        }

        let mut seen = vec![false; section_count];
        let mut final_order = Vec::with_capacity(section_count);
        for id in order {
            let Ok(index) = usize::try_from(id) else {
                continue;
            };
            if index < section_count && !seen[index] {
                seen[index] = true;
                final_order.push(id);
            }
        }
        for (index, was_seen) in seen.iter().enumerate() {
            if !was_seen && let Ok(id) = i32::try_from(index) {
                final_order.push(id);
            }
        }
        self.values.insert(
            "section_order_vector".to_owned(),
            Value::Array(
                final_order.into_iter().map(|value| Value::Number(Number::from(value))).collect(),
            ),
        );
    }

    fn apply_extra_instances(&mut self, settings: &Map<String, Value>) {
        if let Some(items) = settings.get("extra_wallhop_instances").and_then(Value::as_array) {
            let normalized: Vec<Value> = items
                .iter()
                .filter_map(Value::as_object)
                .map(|item| Value::Object(self.normalize_wallhop_instance(item)))
                .collect();
            if !normalized.is_empty() {
                self.values.insert("extra_wallhop_instances".to_owned(), Value::Array(normalized));
            }
        }
        if let Some(items) = settings.get("extra_presskey_instances").and_then(Value::as_array) {
            let normalized: Vec<Value> = items
                .iter()
                .filter_map(Value::as_object)
                .map(|item| Value::Object(self.normalize_presskey_instance(item)))
                .collect();
            if !normalized.is_empty() {
                self.values.insert("extra_presskey_instances".to_owned(), Value::Array(normalized));
            }
        }
        if let Some(items) = settings.get("extra_spamkey_instances").and_then(Value::as_array) {
            let normalized: Vec<Value> = items
                .iter()
                .filter_map(Value::as_object)
                .map(|item| Value::Object(self.normalize_spamkey_instance(item)))
                .collect();
            if !normalized.is_empty() {
                self.values.insert("extra_spamkey_instances".to_owned(), Value::Array(normalized));
            }
        }
    }

    fn normalize_wallhop_instance(&self, item: &Map<String, Value>) -> Map<String, Value> {
        let mut output = Map::new();
        output.insert(
            "vk_trigger".to_owned(),
            normalize_or_default(item.get("vk_trigger"), NumericKind::Unsigned, 6),
        );
        output.insert(
            "vk_jumpkey".to_owned(),
            normalize_or_default(item.get("vk_jumpkey"), NumericKind::Unsigned, 32),
        );
        output.insert(
            "wallhop_dx".to_owned(),
            normalize_or_default(item.get("wallhop_dx"), NumericKind::Int, 300),
        );
        output.insert(
            "wallhop_dy".to_owned(),
            normalize_or_default(item.get("wallhop_dy"), NumericKind::Int, -300),
        );
        output.insert(
            "wallhop_vertical".to_owned(),
            normalize_or_default(item.get("wallhop_vertical"), NumericKind::Int, 0),
        );
        output.insert(
            "WallhopDelay".to_owned(),
            normalize_or_default(item.get("WallhopDelay"), NumericKind::Int, 19),
        );
        output.insert(
            "WallhopBonusDelay".to_owned(),
            normalize_or_default(item.get("WallhopBonusDelay"), NumericKind::Int, 0),
        );
        for (key, default) in [
            ("WallhopPixels", "300"),
            ("WallhopVerticalChar", "0"),
            ("WallhopDelayChar", "19"),
            ("WallhopBonusDelayChar", "0"),
            ("WallhopDegrees", "150"),
        ] {
            output.insert(
                key.to_owned(),
                Value::String(normalize_instance_string(item.get(key), default)),
            );
        }
        for (key, default) in [
            ("wallhopswitch", false),
            ("toggle_jump", true),
            ("toggle_flick", true),
            ("wallhopcamfix", false),
            ("section_enabled", false),
        ] {
            output.insert(
                key.to_owned(),
                Value::Bool(item.get(key).and_then(Value::as_bool).unwrap_or(default)),
            );
        }
        output.insert(
            "disable_outside_roblox".to_owned(),
            Value::Bool(
                item.get("disable_outside_roblox")
                    .and_then(Value::as_bool)
                    .unwrap_or_else(|| self.disable_index(6)),
            ),
        );
        output
    }

    fn normalize_presskey_instance(&self, item: &Map<String, Value>) -> Map<String, Value> {
        let mut output = Map::new();
        output.insert(
            "vk_trigger".to_owned(),
            normalize_or_default(item.get("vk_trigger"), NumericKind::Unsigned, 0x5a),
        );
        output.insert(
            "vk_presskey".to_owned(),
            normalize_or_default(item.get("vk_presskey"), NumericKind::Unsigned, 0x44),
        );
        output.insert(
            "PressKeyDelay".to_owned(),
            normalize_or_default(item.get("PressKeyDelay"), NumericKind::Int, 16),
        );
        output.insert(
            "PressKeyBonusDelay".to_owned(),
            normalize_or_default(item.get("PressKeyBonusDelay"), NumericKind::Int, 0),
        );
        output.insert(
            "PressKeyDelayChar".to_owned(),
            Value::String(normalize_instance_string(item.get("PressKeyDelayChar"), "16")),
        );
        output.insert(
            "PressKeyBonusDelayChar".to_owned(),
            Value::String(normalize_instance_string(item.get("PressKeyBonusDelayChar"), "0")),
        );
        let outside = item
            .get("disable_outside_roblox")
            .and_then(Value::as_bool)
            .or_else(|| item.get("presskeyinroblox").and_then(Value::as_bool))
            .unwrap_or_else(|| self.disable_index(5));
        output.insert("disable_outside_roblox".to_owned(), Value::Bool(outside));
        output.insert("presskeyinroblox".to_owned(), Value::Bool(outside));
        output.insert(
            "section_enabled".to_owned(),
            Value::Bool(item.get("section_enabled").and_then(Value::as_bool).unwrap_or(false)),
        );
        output
    }

    fn normalize_spamkey_instance(&self, item: &Map<String, Value>) -> Map<String, Value> {
        let mut output = Map::new();
        output.insert(
            "vk_trigger".to_owned(),
            normalize_or_default(item.get("vk_trigger"), NumericKind::Unsigned, 0xdb),
        );
        output.insert(
            "vk_spamkey".to_owned(),
            normalize_or_default(item.get("vk_spamkey"), NumericKind::Unsigned, 1),
        );
        output.insert(
            "spam_delay".to_owned(),
            normalize_or_default(item.get("spam_delay"), NumericKind::Float, 20),
        );
        output.insert(
            "real_delay".to_owned(),
            normalize_or_default(item.get("real_delay"), NumericKind::Int, 1000),
        );
        output.insert(
            "SpamDelay".to_owned(),
            Value::String(normalize_instance_string(item.get("SpamDelay"), "20")),
        );
        output.insert(
            "isspamswitch".to_owned(),
            Value::Bool(item.get("isspamswitch").and_then(Value::as_bool).unwrap_or(false)),
        );
        output.insert(
            "disable_outside_roblox".to_owned(),
            Value::Bool(
                item.get("disable_outside_roblox")
                    .and_then(Value::as_bool)
                    .unwrap_or_else(|| self.disable_index(11)),
            ),
        );
        output.insert(
            "section_enabled".to_owned(),
            Value::Bool(item.get("section_enabled").and_then(Value::as_bool).unwrap_or(false)),
        );
        output
    }

    fn clamp_selected_wallhop(&mut self) {
        let extra_count = self
            .values
            .get("extra_wallhop_instances")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let instance_count = extra_count + 1;
        let selected =
            self.values.get("selected_wallhop_instance").and_then(Value::as_i64).unwrap_or(0);
        let selected_i32 = i32::try_from(selected).unwrap_or_default();
        let clamped = if selected_i32 < 0 {
            0
        } else {
            let selected_index = usize::try_from(selected_i32).unwrap_or_default();
            if selected_index >= instance_count {
                i32::try_from(instance_count - 1).unwrap_or_default()
            } else {
                selected_i32
            }
        };
        self.values
            .insert("selected_wallhop_instance".to_owned(), Value::Number(Number::from(clamped)));
    }
}

/// Validates a profile payload using the same optional-field/type rules as C++.
#[must_use]
pub fn validate(payload: &Value) -> bool {
    let Some(settings) = payload.as_object() else {
        return false;
    };
    let Some(specification) = specification() else {
        return false;
    };
    let schema = &specification.schema;

    if schema.bool_keys.iter().any(|key| settings.get(key).is_some_and(|value| !value.is_boolean()))
    {
        return false;
    }
    if schema
        .numeric_kinds
        .keys()
        .any(|key| settings.get(key).is_some_and(|value| !value.is_number()))
    {
        return false;
    }
    if schema
        .char_buffers
        .keys()
        .any(|key| settings.get(key).is_some_and(|value| !value.is_string()))
    {
        return false;
    }
    if settings.get("text").is_some_and(|value| !value.is_string()) {
        return false;
    }
    for key in ["screen_width", "screen_height"] {
        if settings.get(key).is_some_and(|value| !is_json_integer(value)) {
            return false;
        }
    }
    for key in ["section_toggles", "disable_outside_roblox"] {
        if let Some(value) = settings.get(key) {
            let Some(array) = value.as_array() else {
                return false;
            };
            if array.iter().any(|item| !item.is_boolean()) {
                return false;
            }
        }
    }
    if let Some(value) = settings.get("section_order_vector") {
        let Some(array) = value.as_array() else {
            return false;
        };
        if array.iter().any(|item| !is_json_integer(item)) {
            return false;
        }
    }

    validate_extra_array(settings.get("extra_wallhop_instances"), validate_wallhop_instance)
        && validate_extra_array(
            settings.get("extra_presskey_instances"),
            validate_presskey_instance,
        )
        && validate_extra_array(settings.get("extra_spamkey_instances"), validate_spamkey_instance)
        && validate_imported_scripts(settings.get("imported_scripts"))
}

fn validate_extra_array(
    value: Option<&Value>,
    validate_item: fn(&Map<String, Value>) -> bool,
) -> bool {
    let Some(value) = value else {
        return true;
    };
    let Some(array) = value.as_array() else {
        return false;
    };
    array.iter().all(|item| item.as_object().is_some_and(validate_item))
}

fn validate_wallhop_instance(item: &Map<String, Value>) -> bool {
    validate_unsigned_keys(item, &["vk_trigger", "vk_jumpkey"])
        && validate_number_keys(
            item,
            &["wallhop_dx", "wallhop_dy", "wallhop_vertical", "WallhopDelay", "WallhopBonusDelay"],
        )
        && validate_bool_keys(
            item,
            &[
                "wallhopswitch",
                "toggle_jump",
                "toggle_flick",
                "wallhopcamfix",
                "disable_outside_roblox",
                "section_enabled",
            ],
        )
        && validate_string_keys(
            item,
            &[
                "WallhopPixels",
                "WallhopVerticalChar",
                "WallhopDelayChar",
                "WallhopBonusDelayChar",
                "WallhopDegrees",
            ],
        )
}

fn validate_presskey_instance(item: &Map<String, Value>) -> bool {
    validate_unsigned_keys(item, &["vk_trigger", "vk_presskey"])
        && validate_number_keys(item, &["PressKeyDelay", "PressKeyBonusDelay"])
        && validate_string_keys(item, &["PressKeyDelayChar", "PressKeyBonusDelayChar"])
        && validate_bool_keys(
            item,
            &["disable_outside_roblox", "presskeyinroblox", "section_enabled"],
        )
}

fn validate_spamkey_instance(item: &Map<String, Value>) -> bool {
    validate_unsigned_keys(item, &["vk_trigger", "vk_spamkey"])
        && validate_number_keys(item, &["spam_delay", "real_delay"])
        && validate_string_keys(item, &["SpamDelay"])
        && validate_bool_keys(item, &["isspamswitch", "disable_outside_roblox", "section_enabled"])
}

fn validate_imported_scripts(value: Option<&Value>) -> bool {
    let Some(value) = value else {
        return true;
    };
    let Some(array) = value.as_array() else {
        return false;
    };
    array.iter().all(|item| {
        let Some(object) = item.as_object() else {
            return false;
        };
        if object.get("path").and_then(Value::as_str).is_none_or(str::is_empty) {
            return false;
        }
        if object.get("hotkey").is_some_and(|hotkey| !is_valid_unsigned_setting(hotkey)) {
            return false;
        }
        for key in ["enabled", "disable_outside_roblox"] {
            if object.get(key).is_some_and(|setting| !setting.is_boolean()) {
                return false;
            }
        }
        object.get("ui_state").is_none_or(Value::is_object)
    })
}

fn validate_unsigned_keys(item: &Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().all(|key| item.get(*key).is_none_or(is_valid_unsigned_setting))
}

fn validate_number_keys(item: &Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().all(|key| item.get(*key).is_none_or(Value::is_number))
}

fn validate_bool_keys(item: &Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().all(|key| item.get(*key).is_none_or(Value::is_boolean))
}

fn validate_string_keys(item: &Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().all(|key| item.get(*key).is_none_or(Value::is_string))
}

fn is_valid_unsigned_setting(value: &Value) -> bool {
    value.as_u64().is_some()
}

fn is_json_integer(value: &Value) -> bool {
    value.as_i64().is_some() || value.as_u64().is_some()
}

fn normalize_numeric(value: &Value, kind: NumericKind) -> Option<Value> {
    match kind {
        NumericKind::Int => normalize_i32(value).map(|number| Value::Number(Number::from(number))),
        NumericKind::Unsigned => {
            normalize_u32(value).map(|number| Value::Number(Number::from(number)))
        }
        NumericKind::Float => {
            value.as_f64().map(cpp_f64_to_f32).and_then(|number| float_value(f64::from(number)))
        }
    }
}

fn normalize_i32(value: &Value) -> Option<i32> {
    if let Some(number) = value.as_i64() {
        let modulo = number.rem_euclid(1_i64 << 32);
        return u32::try_from(modulo).ok().map(u32::cast_signed);
    }
    if let Some(number) = value.as_u64() {
        let modulo = number % (1_u64 << 32);
        return u32::try_from(modulo).ok().map(u32::cast_signed);
    }
    value.as_f64().map(cpp_f64_to_i32)
}

fn normalize_u32(value: &Value) -> Option<u32> {
    if let Some(number) = value.as_i64() {
        let modulo = number.rem_euclid(1_i64 << 32);
        return u32::try_from(modulo).ok();
    }
    if let Some(number) = value.as_u64() {
        let modulo = number % (1_u64 << 32);
        return u32::try_from(modulo).ok();
    }
    value.as_f64().map(cpp_f64_to_u32)
}

#[allow(clippy::cast_possible_truncation)]
const fn cpp_f64_to_f32(number: f64) -> f32 {
    // Mirrors nlohmann::json -> float narrowing in the C++ settings loader.
    number as f32
}

#[allow(clippy::cast_possible_truncation)]
const fn cpp_f64_to_i32(number: f64) -> i32 {
    // C++ profile loading narrows number_float directly to int. The oracle freezes the behavior
    // for representable compatibility cases; out-of-range floating-point conversion is not relied on.
    number as i32
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
const fn cpp_f64_to_u32(number: f64) -> u32 {
    // Same compatibility note as `cpp_f64_to_i32`; validation intentionally permits fractions.
    number as u32
}

#[allow(clippy::manual_midpoint)]
fn cpp_spam_real_delay(parsed: f32) -> Option<i32> {
    // Preserve the exact arithmetic topology in SyncRuntimeSettingsFromBuffersImpl.
    let real_delay = (f64::from(parsed) + 0.5) / 2.0;
    if real_delay < f64::from(i32::MIN) || real_delay > f64::from(i32::MAX) {
        return None;
    }
    Some(cpp_f64_to_i32(real_delay))
}

fn float_value(number: f64) -> Option<Value> {
    Number::from_f64(number).map(Value::Number)
}

fn normalize_or_default(value: Option<&Value>, kind: NumericKind, default: i32) -> Value {
    value.and_then(|setting| normalize_numeric(setting, kind)).unwrap_or_else(|| match kind {
        NumericKind::Float => float_value(f64::from(default)).unwrap_or(Value::Null),
        NumericKind::Unsigned => {
            let unsigned = u32::try_from(default).unwrap_or_default();
            Value::Number(Number::from(unsigned))
        }
        NumericKind::Int => Value::Number(Number::from(default)),
    })
}

fn truncate_utf8_buffer(value: &str, capacity: usize) -> String {
    if capacity == 0 {
        return String::new();
    }
    let maximum_bytes = capacity - 1;
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut boundary = maximum_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

fn normalize_instance_string(value: Option<&Value>, default: &str) -> String {
    truncate_utf8_buffer(
        value.and_then(Value::as_str).unwrap_or(default),
        DEFAULT_INSTANCE_BUFFER_CAPACITY,
    )
}

fn parse_cpp_i32(value: &str) -> Option<i32> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let digits = if bytes[0] == b'-' {
        if bytes.len() == 1 {
            return None;
        }
        &bytes[1..]
    } else {
        bytes
    };
    if digits.is_empty() || digits.iter().any(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<i32>().ok()
}

fn parse_cpp_f32(value: &str) -> Option<f32> {
    // `strtof` skips leading C whitespace but requires the parse end to equal the NUL terminator.
    let trimmed_start = value.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let parsed = trimmed_start.parse::<f32>().ok()?;
    parsed.is_finite().then_some(parsed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{ProfilePayloadState, validate};
    use serde::Deserialize;
    use serde_json::Value;

    #[derive(Debug, Deserialize)]
    struct Fixture {
        default_payload: Value,
        validation_cases: Vec<ValidationCase>,
        apply_cases: Vec<ApplyCase>,
        sequence_cases: Vec<SequenceCase>,
    }

    #[derive(Debug, Deserialize)]
    struct ValidationCase {
        name: String,
        payload: Value,
        valid: bool,
    }

    #[derive(Debug, Deserialize)]
    struct ApplyCase {
        name: String,
        payload: Value,
        success: bool,
        result: Value,
    }

    #[derive(Debug, Deserialize)]
    struct SequenceCase {
        name: String,
        payloads: Vec<Value>,
        success: bool,
        steps: Vec<SequenceStep>,
    }

    #[derive(Debug, Deserialize)]
    struct SequenceStep {
        success: bool,
        payload: Value,
    }

    fn fixture() -> Fixture {
        serde_json::from_str(super::PAYLOAD_FIXTURE)
            .expect("C++ generated payload compatibility fixture should parse")
    }

    #[test]
    fn cpp_default_payload_matches_rust_default() {
        let fixture = fixture();
        assert_eq!(ProfilePayloadState::default().snapshot(), fixture.default_payload);
    }

    #[test]
    fn cpp_generated_validation_cases_match() {
        for case in fixture().validation_cases {
            assert_eq!(validate(&case.payload), case.valid, "case {}", case.name);
        }
    }

    #[test]
    fn cpp_generated_apply_cases_match() {
        for case in fixture().apply_cases {
            let mut state = ProfilePayloadState::default();
            let success = state.apply(&case.payload);
            assert_eq!(success, case.success, "case {} success", case.name);
            assert_eq!(state.snapshot(), case.result, "case {} payload", case.name);
        }
    }

    #[test]
    fn cpp_generated_stateful_sequences_match() {
        for case in fixture().sequence_cases {
            let mut state = ProfilePayloadState::default();
            let mut all_success = true;
            assert_eq!(case.payloads.len(), case.steps.len(), "case {} fixture shape", case.name);
            for (index, (payload, expected)) in case.payloads.iter().zip(&case.steps).enumerate() {
                let success = state.apply(payload);
                all_success &= success;
                assert_eq!(success, expected.success, "case {} step {} success", case.name, index);
                assert_eq!(
                    state.snapshot(),
                    expected.payload,
                    "case {} step {} payload",
                    case.name,
                    index
                );
            }
            assert_eq!(all_success, case.success, "case {} aggregate success", case.name);
        }
    }
}
