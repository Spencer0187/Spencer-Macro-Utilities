#![allow(clippy::expect_used)]

use serde::Deserialize;
use smu_core::{Hotkey, KeyCode};
use smu_input::{HotkeyMatch, KeyState, is_hotkey_pressed};
use std::collections::BTreeSet;

const FIXTURE: &str = include_str!("../../../../compat/input/hotkey_cases.json");

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    hotkey: u32,
    strict: bool,
    pressed: Vec<u32>,
    result: bool,
}

struct FixtureState(BTreeSet<KeyCode>);

impl KeyState for FixtureState {
    fn is_pressed(&self, key: KeyCode) -> bool {
        self.0.contains(&key)
    }
}

#[test]
fn cpp_hotkey_matching_cases_match() {
    let fixture: Fixture =
        serde_json::from_str(FIXTURE).expect("C++ generated input-hotkey fixture should parse");
    for case in fixture.cases {
        let state = FixtureState(case.pressed.into_iter().map(KeyCode::new).collect());
        let mode = if case.strict { HotkeyMatch::Strict } else { HotkeyMatch::Loose };
        assert_eq!(
            is_hotkey_pressed(&state, Hotkey::new(case.hotkey), mode),
            case.result,
            "case {}",
            case.name
        );
    }
}
