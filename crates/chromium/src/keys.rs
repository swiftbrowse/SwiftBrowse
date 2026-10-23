//! Key names to CDP `Input.dispatchKeyEvent` parameters.

use serde_json::{json, Value};
use swiftbrowse_core::{CoreError, Result};

pub struct KeyStroke {
    pub down: Value,
    pub up: Value,
}

const ALT: i64 = 1;
const CTRL: i64 = 2;
const META: i64 = 4;
const SHIFT: i64 = 8;

/// Parses `Enter`, `a`, `Control+Shift+K` and similar.
pub fn parse(chord: &str) -> Result<KeyStroke> {
    let parts: Vec<&str> = if chord == "+" { vec!["+"] } else { chord.split('+').collect() };
    let (key, mods) = parts.split_last().ok_or_else(|| CoreError::invalid("empty key"))?;
    let mut modifiers = 0;
    for m in mods {
        modifiers |= match m.to_ascii_lowercase().as_str() {
            "alt" | "option" => ALT,
            "control" | "ctrl" => CTRL,
            "meta" | "cmd" | "command" => META,
            "shift" => SHIFT,
            other => return Err(CoreError::invalid(format!("unknown modifier {other:?}"))),
        };
    }
    let (key, code, vk, text) = describe(key)?;
    let text = if modifiers & (CTRL | META | ALT) != 0 { None } else { text };
    let mut down = json!({
        "type": if text.is_some() { "keyDown" } else { "rawKeyDown" },
        "key": key, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
        "modifiers": modifiers,
    });
    if let Some(t) = &text {
        down["text"] = json!(t);
        down["unmodifiedText"] = json!(t);
    }
    let up = json!({ "type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk, "modifiers": modifiers });
    Ok(KeyStroke { down, up })
}

fn describe(key: &str) -> Result<(String, String, i64, Option<String>)> {
    let named = |k: &str, code: &str, vk: i64, text: Option<&str>| Ok((k.to_string(), code.to_string(), vk, text.map(str::to_string)));
    match key {
        "Enter" => named("Enter", "Enter", 13, Some("\r")),
        "Tab" => named("Tab", "Tab", 9, None),
        "Escape" | "Esc" => named("Escape", "Escape", 27, None),
        "Backspace" => named("Backspace", "Backspace", 8, None),
        "Delete" => named("Delete", "Delete", 46, None),
        "ArrowLeft" => named("ArrowLeft", "ArrowLeft", 37, None),
        "ArrowUp" => named("ArrowUp", "ArrowUp", 38, None),
        "ArrowRight" => named("ArrowRight", "ArrowRight", 39, None),
        "ArrowDown" => named("ArrowDown", "ArrowDown", 40, None),
        "Home" => named("Home", "Home", 36, None),
        "End" => named("End", "End", 35, None),
        "PageUp" => named("PageUp", "PageUp", 33, None),
        "PageDown" => named("PageDown", "PageDown", 34, None),
        "Space" | " " => named(" ", "Space", 32, Some(" ")),
        k if k.len() > 1 && k.starts_with('F') && k[1..].parse::<i64>().is_ok_and(|n| (1..=12).contains(&n)) => {
            let n: i64 = k[1..].parse().unwrap_or(1);
            named(k, k, 111 + n, None)
        }
        k if k.chars().count() == 1 => {
            let c = k.chars().next().unwrap_or(' ');
            let (code, vk) = if c.is_ascii_alphabetic() {
                (format!("Key{}", c.to_ascii_uppercase()), c.to_ascii_uppercase() as i64)
            } else if c.is_ascii_digit() {
                (format!("Digit{c}"), c as i64)
            } else {
                (String::new(), 0)
            };
            Ok((c.to_string(), code, vk, Some(c.to_string())))
        }
        other => Err(CoreError::invalid(format!("unknown key {other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_and_chords() {
        let k = parse("Enter").unwrap();
        assert_eq!(k.down["text"], "\r");
        let k = parse("Control+a").unwrap();
        assert_eq!(k.down["modifiers"], CTRL);
        assert!(k.down.get("text").is_none());
        assert_eq!(k.down["code"], "KeyA");
        assert!(parse("Hyper+a").is_err());
        assert!(parse("NotAKey").is_err());
        assert_eq!(parse("F5").unwrap().down["windowsVirtualKeyCode"], 116);
    }
}
