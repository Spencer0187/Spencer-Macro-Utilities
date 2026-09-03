//! Imported-script hotkey grammar used by `@keybind` metadata.

use smu_core::{Hotkey, KeyCode};

const MASK_SHIFT: u32 = 0x1_0000;
const MASK_CTRL: u32 = 0x2_0000;
const MASK_ALT: u32 = 0x4_0000;
const MASK_WIN: u32 = 0x8_0000;
const KEY_MASK: u32 = 0xFFFF;

fn normalized(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|character| !matches!(character, ' ' | '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Parses the key-name grammar from `ParseScriptKeyName` in the C++ oracle.
#[must_use]
pub fn parse_script_key_name(text: &str) -> Option<KeyCode> {
    let key = normalized(text);
    if key.is_empty() {
        return None;
    }

    if key.len() == 1 {
        let byte = key.as_bytes()[0].to_ascii_uppercase();
        if byte.is_ascii_uppercase() || byte.is_ascii_digit() {
            return Some(KeyCode::new(u32::from(byte)));
        }
    }

    if let Some(number) = key.strip_prefix('f')
        && number.len() <= 2
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && let Ok(number) = number.parse::<u32>()
        && (1..=24).contains(&number)
    {
        return Some(KeyCode::new(KeyCode::F1.raw() + number - 1));
    }

    if let Some(number) = key.strip_prefix("numpad")
        && number.len() == 1
        && let Some(digit) = number.as_bytes().first().copied()
        && digit.is_ascii_digit()
    {
        return Some(KeyCode::new(KeyCode::NUMPAD0.raw() + u32::from(digit - b'0')));
    }

    let code = match key.as_str() {
        "lmb" | "mouseleft" | "leftmouse" => KeyCode::LBUTTON,
        "rmb" | "mouseright" | "rightmouse" => KeyCode::RBUTTON,
        "mmb" | "mousemiddle" | "middlemouse" => KeyCode::MBUTTON,
        "mouse4" | "xbutton1" => KeyCode::XBUTTON1,
        "mouse5" | "xbutton2" => KeyCode::XBUTTON2,
        "mousewheelup" | "wheelup" => KeyCode::MOUSE_WHEEL_UP,
        "mousewheeldown" | "wheeldown" => KeyCode::MOUSE_WHEEL_DOWN,
        "space" => KeyCode::SPACE,
        "enter" | "return" => KeyCode::RETURN,
        "escape" | "esc" => KeyCode::ESCAPE,
        "tab" => KeyCode::TAB,
        "backspace" => KeyCode::BACK,
        "delete" | "del" => KeyCode::DELETE,
        "insert" | "ins" => KeyCode::INSERT,
        "home" => KeyCode::HOME,
        "end" => KeyCode::END,
        "pageup" | "pgup" => KeyCode::PRIOR,
        "pagedown" | "pgdn" => KeyCode::NEXT,
        "up" => KeyCode::UP,
        "down" => KeyCode::DOWN,
        "left" => KeyCode::LEFT,
        "right" => KeyCode::RIGHT,
        "slash" | "/" => KeyCode::OEM_2,
        "backslash" | "\\" => KeyCode::OEM_5,
        "equals" | "equal" | "=" => KeyCode::OEM_PLUS,
        "minus" | "-" => KeyCode::OEM_MINUS,
        "comma" | "," => KeyCode::OEM_COMMA,
        "period" | "dot" | "." => KeyCode::OEM_PERIOD,
        "semicolon" | ";" => KeyCode::OEM_1,
        "quote" | "apostrophe" | "'" => KeyCode::OEM_7,
        "bracketleft" | "leftbracket" | "[" => KeyCode::OEM_4,
        "bracketright" | "rightbracket" | "]" => KeyCode::OEM_6,
        "grave" | "backtick" | "`" => KeyCode::OEM_3,
        "capslock" => KeyCode::CAPITAL,
        "numlock" => KeyCode::NUMLOCK,
        "scrolllock" => KeyCode::SCROLL,
        "lshift" | "leftshift" => KeyCode::LSHIFT,
        "rshift" | "rightshift" => KeyCode::RSHIFT,
        "shift" => KeyCode::SHIFT,
        "lctrl" | "leftctrl" => KeyCode::LCONTROL,
        "rctrl" | "rightctrl" => KeyCode::RCONTROL,
        "ctrl" | "control" => KeyCode::CONTROL,
        "lalt" | "leftalt" => KeyCode::LMENU,
        "ralt" | "rightalt" => KeyCode::RMENU,
        "alt" => KeyCode::MENU,
        "lwin" | "leftwin" | "win" | "super" | "meta" => KeyCode::LWIN,
        "rwin" | "rightwin" => KeyCode::RWIN,
        _ => return None,
    };
    Some(code)
}

/// Parses the full `modifier+key` grammar from `ParseScriptHotkeyString` in the C++ oracle.
///
/// Empty `+` segments are ignored, side-specific modifiers collapse to generic mask bits, and when
/// multiple main keys are supplied the last one wins.
#[must_use]
pub fn parse_script_hotkey(text: &str) -> Option<Hotkey> {
    let parts: Vec<_> = text.split('+').map(str::trim).filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }

    let mut modifiers = 0_u32;
    let mut main_key = None;
    for part in parts {
        let key = normalized(part);
        match key.as_str() {
            "shift" | "lshift" | "leftshift" | "rshift" | "rightshift" => {
                modifiers |= MASK_SHIFT;
            }
            "ctrl" | "control" | "lctrl" | "leftctrl" | "rctrl" | "rightctrl" => {
                modifiers |= MASK_CTRL;
            }
            "alt" | "lalt" | "leftalt" | "ralt" | "rightalt" => {
                modifiers |= MASK_ALT;
            }
            "win" | "super" | "meta" | "lwin" | "leftwin" | "rwin" | "rightwin" => {
                modifiers |= MASK_WIN;
            }
            _ => {
                main_key = Some(parse_script_key_name(part)?);
            }
        }
    }

    let main_key = main_key?;
    if main_key == KeyCode::NONE {
        return None;
    }
    Some(Hotkey::new((main_key.raw() & KEY_MASK) | modifiers))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::{fs, path::PathBuf};

    use serde::Deserialize;

    use super::parse_script_hotkey;

    #[derive(Debug, Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    #[derive(Debug, Deserialize)]
    struct Case {
        input: String,
        parsed: bool,
        raw: Option<u32>,
    }

    #[test]
    fn hotkey_cases_match_cpp_oracle_fixture() {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let text = fs::read_to_string(repository.join("compat/hotkeys/cases.json"))
            .expect("checked-in C++ oracle fixture should exist");
        let fixture: Fixture =
            serde_json::from_str(&text).expect("checked-in C++ oracle fixture should parse");

        for case in fixture.cases {
            let parsed = parse_script_hotkey(&case.input);
            assert_eq!(parsed.is_some(), case.parsed, "input={:?}", case.input);
            assert_eq!(parsed.map(smu_core::Hotkey::raw), case.raw, "input={:?}", case.input);
        }
    }
}
