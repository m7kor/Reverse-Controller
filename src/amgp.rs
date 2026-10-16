//! AntiMicroX (.amgp) XML Profile Parser & Converter.
//!
//! Converts `.amgp` profile files directly into Controller Companion bindings
//! and mouse/controller settings.

use crate::actions::{Action, Modifier};
use crate::bindings::{Input, PadBindingDef};
use crate::config::{Config, ControllerSettings, MouseSettings};
use crate::keys::Key;
use crate::mouse::MouseButton;
use std::path::Path;

/// Parse an `.amgp` file and generate a corresponding `Config`.
pub fn load_amgp_file(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read amgp file {}: {e}", path.display()))?;
    parse_amgp_xml(&text)
}

/// Write a parsed `.amgp` profile out as a native JSON config.
///
/// Used by the GUI's "convert all" button and the `--convert` CLI flag so an
/// imported profile becomes a first-class, editable Controller Companion
/// profile instead of being re-parsed on every load.
pub fn convert_amgp_file(input: &Path, output: &Path) -> Result<(), String> {
    let cfg = load_amgp_file(input)?;
    cfg.save(output)
        .map_err(|e| format!("Failed to write {}: {e}", output.display()))
}

/// Convert AntiMicroX XML content to a `Config` struct.
pub fn parse_amgp_xml(xml: &str) -> Result<Config, String> {
    if !xml.contains("<gamecontroller") {
        return Err("Not a valid AntiMicroX profile (<gamecontroller> root tag missing)".into());
    }

    let mut bindings = Vec::new();
    let mut mouse = MouseSettings::default();
    let mut controller = ControllerSettings::default();

    // 1. Extract sets
    let set1 = extract_set(xml, 1);
    let set2 = extract_set(xml, 2);

    // 2. Parse Primary Set (Set 1 - Base mappings without modifier)
    if let Some(s1) = &set1 {
        parse_set_buttons(s1, None, &mut bindings);
        parse_set_dpads(s1, None, &mut bindings);
        parse_set_triggers(s1, None, &mut bindings);
        parse_set_sticks(s1, None, &mut bindings, &mut mouse, &mut controller);
    }

    // 3. Parse Secondary Set (Set 2 - Modified layer with Modifier::Back)
    if let Some(s2) = &set2 {
        parse_set_buttons(s2, Some(Modifier::Back), &mut bindings);
        parse_set_dpads(s2, Some(Modifier::Back), &mut bindings);
        parse_set_triggers(s2, Some(Modifier::Back), &mut bindings);
        // Note: sticks in secondary layer can also be parsed if custom
    }

    // Always ensure Back button has Modifier::Back if Set 2 exists and Back was not explicitly mapped
    let has_back_binding = bindings.iter().any(|b| b.input == Input::Back && b.modifier.is_none());
    if !has_back_binding && set2.is_some() {
        bindings.push(PadBindingDef {
            input: Input::Back,
            modifier: None,
            action: Action::Modifier(Modifier::Back),
        });
    }

    // If no bindings were found, return an error
    if bindings.is_empty() {
        return Err("No compatible bindings found in .amgp profile".into());
    }

    // Deterministic sort: modified bindings first (like default_profile)
    bindings.sort_by_key(|b| std::cmp::Reverse(b.modifier.is_some() as u8));

    Ok(Config {
        enabled: true,
        disable_in_fullscreen: false,
        autostart: false,
        vk_auto_show: false,
        mouse,
        controller,
        bindings,
        ..Config::default()
    })
}

fn extract_set(xml: &str, set_idx: u32) -> Option<String> {
    let target_prefix = format!("<set index=\"{}\"", set_idx);
    let start_pos = xml.find(&target_prefix)?;
    let content_start = xml[start_pos..].find('>')? + start_pos + 1;
    let end_pos = xml[content_start..].find("</set>")? + content_start;
    Some(xml[content_start..end_pos].to_string())
}

fn parse_set_buttons(
    set_xml: &str,
    modifier: Option<Modifier>,
    out: &mut Vec<PadBindingDef>,
) {
    let mut cur = 0;
    while let Some(btn_start) = set_xml[cur..].find("<button index=\"") {
        let abs_start = cur + btn_start;
        let idx_str_start = abs_start + "<button index=\"".len();
        let idx_str_end = match set_xml[idx_str_start..].find('\"') {
            Some(p) => idx_str_start + p,
            None => break,
        };
        let btn_idx: u32 = set_xml[idx_str_start..idx_str_end].parse().unwrap_or(0);

        let btn_end = match set_xml[abs_start..].find("</button>") {
            Some(p) => abs_start + p + "</button>".len(),
            None => {
                // Check self-closing <button ... />
                match set_xml[abs_start..].find("/>") {
                    Some(p) => abs_start + p + "/>".len(),
                    None => break,
                }
            }
        };

        let block = &set_xml[abs_start..btn_end];
        cur = btn_end;

        if let Some(input) = map_button_index(btn_idx) {
            // Check if this button activates set 2 as a shift modifier
            if block.contains("<setselect>2</setselect>") && block.contains("while-held") {
                out.push(PadBindingDef {
                    input,
                    modifier,
                    action: Action::Modifier(Modifier::Back),
                });
                continue;
            }

            // If it has normal mouse or keyboard action, parse it first
            if let Some(action) = parse_slots(block) {
                out.push(PadBindingDef { input, modifier, action });
                continue;
            }

            // Check if this button toggles virtual keyboard
            if block.contains("Open Gamepad keyboard") || block.contains("AMXKB") {
                out.push(PadBindingDef {
                    input,
                    modifier,
                    action: Action::ToggleVirtualKeyboard,
                });
                continue;
            }
        }
    }
}

fn parse_set_dpads(
    set_xml: &str,
    modifier: Option<Modifier>,
    out: &mut Vec<PadBindingDef>,
) {
    let mut cur = 0;
    while let Some(start) = set_xml[cur..].find("<dpadbutton index=\"") {
        let abs_start = cur + start;
        let idx_start = abs_start + "<dpadbutton index=\"".len();
        let idx_end = match set_xml[idx_start..].find('\"') {
            Some(p) => idx_start + p,
            None => break,
        };
        let dpad_idx: u32 = set_xml[idx_start..idx_end].parse().unwrap_or(0);

        let end = match set_xml[abs_start..].find("</dpadbutton>") {
            Some(p) => abs_start + p + "</dpadbutton>".len(),
            None => break,
        };
        let block = &set_xml[abs_start..end];
        cur = end;

        let input = match dpad_idx {
            1 => Some(Input::DPadUp),
            2 => Some(Input::DPadRight),
            4 => Some(Input::DPadDown),
            8 => Some(Input::DPadLeft),
            _ => None,
        };

        if let Some(input) = input {
            if let Some(action) = parse_slots(block) {
                out.push(PadBindingDef { input, modifier, action });
            }
        }
    }
}

fn parse_set_triggers(
    set_xml: &str,
    modifier: Option<Modifier>,
    out: &mut Vec<PadBindingDef>,
) {
    let mut cur = 0;
    while let Some(start) = set_xml[cur..].find("<trigger index=\"") {
        let abs_start = cur + start;
        let idx_start = abs_start + "<trigger index=\"".len();
        let idx_end = match set_xml[idx_start..].find('\"') {
            Some(p) => idx_start + p,
            None => break,
        };
        let trig_idx: u32 = set_xml[idx_start..idx_end].parse().unwrap_or(0);

        let end = match set_xml[abs_start..].find("</trigger>") {
            Some(p) => abs_start + p + "</trigger>".len(),
            None => break,
        };
        let block = &set_xml[abs_start..end];
        cur = end;

        // AntiMicroX trigger 5 = LeftTrigger, 6 = RightTrigger (or 1 / 2 in older formats)
        let input = match trig_idx {
            1 | 5 => Some(Input::LeftTrigger),
            2 | 6 => Some(Input::RightTrigger),
            _ => None,
        };

        if let Some(input) = input {
            if let Some(action) = parse_slots(block) {
                out.push(PadBindingDef { input, modifier, action });
            }
        }
    }
}

fn parse_set_sticks(
    set_xml: &str,
    modifier: Option<Modifier>,
    out: &mut Vec<PadBindingDef>,
    mouse: &mut MouseSettings,
    controller: &mut ControllerSettings,
) {
    let mut cur = 0;
    while let Some(start) = set_xml[cur..].find("<stick index=\"") {
        let abs_start = cur + start;
        let idx_start = abs_start + "<stick index=\"".len();
        let idx_end = match set_xml[idx_start..].find('\"') {
            Some(p) => idx_start + p,
            None => break,
        };
        let stick_idx: u32 = set_xml[idx_start..idx_end].parse().unwrap_or(0);

        let end = match set_xml[abs_start..].find("</stick>") {
            Some(p) => abs_start + p + "</stick>".len(),
            None => break,
        };
        let block = &set_xml[abs_start..end];
        cur = end;

        let deadzone = extract_tag_u32(block, "deadZone").unwrap_or(0) as u16;

        if stick_idx == 1 {
            // Left Stick
            if deadzone > 0 {
                controller.left_stick_dead_zone = deadzone;
            }
            if block.contains("<mode>mousemovement</mode>") || block.contains("<mousemode>cursor</mousemode>") {
                out.push(PadBindingDef {
                    input: Input::LeftStick,
                    modifier,
                    action: Action::MouseMove { inverted: false },
                });
                if let Some(sx) = extract_tag_u32(block, "mousespeedx") {
                    // AntiMicroX speed scale (~60-100) maps roughly to 500-800 px/s
                    mouse.speed = (sx as f32 * 10.0).clamp(200.0, 2000.0);
                }
                // AntiMicroX only exposes one speed scale, but the port supports
                // independent horizontal and vertical rates. Mirror the single
                // value into both so an imported profile keeps its feel.
                if let Some(sy) = extract_tag_u32(block, "mousespeedy") {
                    mouse.speed_y = (sy as f32 * 10.0).clamp(0.0, 4000.0);
                }
                apply_shaping(block, mouse);
            }
        } else if stick_idx == 2 {
            // Right Stick
            if deadzone > 0 {
                controller.right_stick_dead_zone = deadzone;
            }
            if block.contains("mousebutton") && (block.contains("Scroll") || block.contains("wheelspeed")) {
                out.push(PadBindingDef {
                    input: Input::RightStick,
                    modifier,
                    action: Action::MouseScroll { flip_y: false },
                });
                if let Some(wy) = extract_tag_u32(block, "wheelspeedy") {
                    mouse.wheel_speed = (wy as f32 * 400.0).clamp(1000.0, 10000.0);
                }
                if let Some(wx) = extract_tag_u32(block, "wheelspeedx") {
                    mouse.wheel_speed_x = (wx as f32 * 400.0).clamp(0.0, 20000.0);
                }
                apply_shaping(block, mouse);
            } else if block.contains("<mode>mousemovement</mode>") {
                out.push(PadBindingDef {
                    input: Input::RightStick,
                    modifier,
                    action: Action::MouseMove { inverted: false },
                });
            }
        }
    }
}

fn map_button_index(idx: u32) -> Option<Input> {
    match idx {
        1 => Some(Input::A),
        2 => Some(Input::B),
        3 => Some(Input::X),
        4 => Some(Input::Y),
        5 => Some(Input::LeftShoulder),
        6 => Some(Input::RightShoulder),
        7 => Some(Input::Back),
        8 => Some(Input::Start),
        9 => Some(Input::LeftThumb),
        10 => Some(Input::RightThumb),
        11 => Some(Input::Guide),
        _ => None,
    }
}

fn parse_slots(block: &str) -> Option<Action> {
    // 1. Check for mouse buttons
    if block.contains("<mode>mousebutton</mode>") {
        if let Some(code) = extract_slot_code(block, "mousebutton") {
            let mb = match code {
                1 => MouseButton::Left,
                2 => MouseButton::Middle,
                3 => MouseButton::Right,
                4 => return Some(Action::MouseScroll { flip_y: false }),
                5 => return Some(Action::MouseScroll { flip_y: false }),
                _ => MouseButton::Left,
            };
            return Some(Action::MouseClick(mb));
        }
    }

    // 2. Check for keyboard keys
    if block.contains("<mode>keyboard</mode>") {
        let keys = extract_all_keyboard_codes(block);
        if keys.len() == 1 {
            let key = keys[0];
            let repeat = matches!(
                key,
                Key::Left | Key::Right | Key::Up | Key::Down | Key::Backspace | Key::Space
            );
            return Some(Action::Key { key, repeat });
        } else if keys.len() > 1 {
            // Check for special combo patterns like Win + M
            if keys.contains(&Key::LWin) && keys.contains(&Key::M) {
                return Some(Action::Combo(vec![Key::LWin, Key::M]));
            }
            if keys.contains(&Key::LControl) && keys.contains(&Key::LMenu) && keys.contains(&Key::Tab) {
                return Some(Action::Combo(vec![Key::LControl, Key::LMenu, Key::Tab]));
            }
            return Some(Action::Combo(keys));
        }
    }

    None
}

/// Read the stick-shaping tags shared by every stick block.
///
/// These come from the reference `v39 Smooth Native Scroll` profile and are
/// what give the pointer its speed ramp and precision ring.
fn apply_shaping(block: &str, mouse: &mut crate::config::MouseSettings) {
    if let Some(v) = extract_tag_u32(block, "modifierZone") {
        mouse.modifier_zone = v as f32;
    }
    if let Some(v) = extract_tag_u32(block, "maxZone") {
        mouse.max_zone = (v as f32).clamp(8000.0, 32767.0);
    }
    if let Some(v) = extract_tag_u32(block, "diagonalRange") {
        // The reference value (80) is a percentage of a 32767 rail, which here
        // would quantise most of the travel and mangle the direction of
        // anything but a deliberate 45-degree push.
        mouse.diagonal_range = (v as f32 * 0.15).clamp(0.0, 100.0);
    }
    if block.contains("<mode>four-way</mode>") {
        mouse.scroll_four_way = true;
    }
}

fn extract_slot_code(block: &str, mode: &str) -> Option<u32> {
    let mode_str = format!("<mode>{}</mode>", mode);
    let mut cur = 0;
    while let Some(slot_start) = block[cur..].find("<slot>") {
        let abs_start = cur + slot_start;
        let slot_end = match block[abs_start..].find("</slot>") {
            Some(p) => abs_start + p + "</slot>".len(),
            None => break,
        };
        let slot_block = &block[abs_start..slot_end];
        cur = slot_end;

        if slot_block.contains(&mode_str) {
            return extract_tag_u32(slot_block, "code");
        }
    }
    None
}

fn extract_all_keyboard_codes(block: &str) -> Vec<Key> {
    let mut out = Vec::new();
    let mut cur = 0;
    while let Some(slot_start) = block[cur..].find("<slot>") {
        let abs_start = cur + slot_start;
        let slot_end = match block[abs_start..].find("</slot>") {
            Some(p) => abs_start + p + "</slot>".len(),
            None => break,
        };
        let slot_block = &block[abs_start..slot_end];
        cur = slot_end;

        if slot_block.contains("<mode>keyboard</mode>") {
            if let Some(code_val) = extract_tag_hex_or_dec(slot_block, "code") {
                if let Some(k) = map_antimicro_code_to_key(code_val) {
                    out.push(k);
                }
            }
        }
    }
    out
}

fn map_antimicro_code_to_key(code: u32) -> Option<Key> {
    // Standard Qt / AntiMicroX keycode table:
    match code {
        // ASCII alphanumeric
        0x20 => Some(Key::Space),
        0x30..=0x39 => {
            let digit = (code - 0x30) as u8;
            match digit {
                0 => Some(Key::D0),
                1 => Some(Key::D1),
                2 => Some(Key::D2),
                3 => Some(Key::D3),
                4 => Some(Key::D4),
                5 => Some(Key::D5),
                6 => Some(Key::D6),
                7 => Some(Key::D7),
                8 => Some(Key::D8),
                9 => Some(Key::D9),
                _ => None,
            }
        }
        0x41..=0x5A => {
            // A-Z
            let k = match (code - 0x41) as u8 {
                0 => Key::A, 1 => Key::B, 2 => Key::C, 3 => Key::D, 4 => Key::E,
                5 => Key::F, 6 => Key::G, 7 => Key::H, 8 => Key::I, 9 => Key::J,
                10 => Key::K, 11 => Key::L, 12 => Key::M, 13 => Key::N, 14 => Key::O,
                15 => Key::P, 16 => Key::Q, 17 => Key::R, 18 => Key::S, 19 => Key::T,
                20 => Key::U, 21 => Key::V, 22 => Key::W, 23 => Key::X, 24 => Key::Y,
                25 => Key::Z,
                _ => return None,
            };
            Some(k)
        }
        // Qt Virtual Keys (0x01000000 base)
        0x1000000 => Some(Key::Escape),
        0x1000001 => Some(Key::Tab),
        0x1000003 => Some(Key::Backspace),
        0x1000004 | 0x1000005 => Some(Key::Enter),
        0x1000006 => Some(Key::Insert),
        0x1000007 => Some(Key::Delete),
        0x1000008 => Some(Key::Pause),
        0x1000009 => Some(Key::PrintScreen),
        0x1000010 => Some(Key::Home),
        0x1000011 => Some(Key::End),
        0x1000012 => Some(Key::Left),
        0x1000013 => Some(Key::Up),
        0x1000014 => Some(Key::Right),
        0x1000015 => Some(Key::Down),
        0x1000016 => Some(Key::PageUp),
        0x1000017 => Some(Key::PageDown),
        0x1000020 => Some(Key::LShift),
        0x1000021 => Some(Key::LControl),
        0x1000022 => Some(Key::LWin),
        0x1000023 => Some(Key::LMenu), // Alt
        0x1000024 => Some(Key::CapsLock),
        0x1000025 => Some(Key::NumLock),
        0x1000026 => Some(Key::ScrollLock),
        // F1-F12
        0x1000030 => Some(Key::F1),
        0x1000031 => Some(Key::F2),
        0x1000032 => Some(Key::F3),
        0x1000033 => Some(Key::F4),
        0x1000034 => Some(Key::F5),
        0x1000035 => Some(Key::F6),
        0x1000036 => Some(Key::F7),
        0x1000037 => Some(Key::F8),
        0x1000038 => Some(Key::F9),
        0x1000039 => Some(Key::F10),
        0x100003a => Some(Key::F11),
        0x100003b => Some(Key::F12),
        // Multimedia keys
        0x1000070 => Some(Key::VolumeDown),
        0x1000071 => Some(Key::VolumeMute),
        0x1000072 => Some(Key::VolumeUp),
        0x1000080 => Some(Key::MediaPlayPause),
        0x1000081 => Some(Key::MediaStop),
        0x1000082 => Some(Key::MediaPreviousTrack),
        0x1000083 => Some(Key::MediaNextTrack),
        0x1000086 => Some(Key::MediaPlayPause),
        _ => None,
    }
}

fn extract_tag_u32(block: &str, tag: &str) -> Option<u32> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    block[start..end].trim().parse().ok()
}

fn extract_tag_hex_or_dec(block: &str, tag: &str) -> Option<u32> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    let s = block[start..end].trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        u32::from_str_radix(&s[2..], 16).ok()
    } else {
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_amgp_sample_profile() {
        let sample_path = Path::new("c:/Users/KH/Downloads/Controller-Companion-Desktop-v39-Smooth-Native-Scroll.gamecontroller (2).amgp");
        if sample_path.exists() {
            let cfg = load_amgp_file(sample_path).expect("Failed to parse user amgp");
            assert!(!cfg.bindings.is_empty(), "Expected bindings parsed from amgp");
            // Check that left stick is mouse movement
            assert!(cfg.bindings.iter().any(|b| b.input == Input::LeftStick && matches!(b.action, Action::MouseMove { .. })));
            // Check that right stick scrolls
            assert!(cfg.bindings.iter().any(|b| b.input == Input::RightStick && matches!(b.action, Action::MouseScroll { .. })));
            // Check that Button 1 (A) has Left click
            assert!(cfg.bindings.iter().any(|b| b.input == Input::A && matches!(b.action, Action::MouseClick(MouseButton::Left))));
            // Check that DPad has arrows
            assert!(cfg.bindings.iter().any(|b| b.input == Input::DPadUp && matches!(b.action, Action::Key { key: Key::Up, .. })));
        }
    }
}
