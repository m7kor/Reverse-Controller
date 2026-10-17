# Reverse Controller — masterpiece plan (v1.3)

## Product goal

Replace the physical mouse and keyboard for everyday Windows desktop use.
After launching the app, a user can set up, navigate, click, scroll, type in
Arabic and English, and recover from interruptions using only the controller.
Movement is predictable, and the app always shows whether desktop control is
active, paused, or unavailable.

## Brainstorm — full read of the code base (18 October 2026)

Baseline: 125 full-feature tests and 89 headless tests pass; strict Clippy is
clean. The code is in good shape structurally, but a full read found features
that are wired up in the UI and docs yet never reach the engine, plus several
ways runtime state can leak into the saved configuration.

### A. Confirmed defects (fix first)

| # | Area | Defect | Effect on the user |
|---|------|--------|--------------------|
| A1 | Persistence | On exit the whole runtime config is saved, including bindings from an auto-switched game profile. | Quitting while a game profile is active overwrites the base bindings. |
| A2 | Profiles | Auto-switch returns to *Default* when no rule matches, not to the profile the user picked; after a config reload it never re-applies. | A profile chosen in Settings is lost on the next focus change. |
| A3 | Actions | `ActivateProfile` only shows the OSD; nothing reads `pending_profile`. | Profile-switch buttons do nothing. |
| A4 | Actions | `NextVirtualDesktop` / `PrevVirtualDesktop` have no handler; `VoiceDictation` is an empty arm. | Three bindable actions do nothing. |
| A5 | Config | The object form of `WindowDrag` is rejected by the hand-written deserializer. | A hand-edited profile fails to load entirely. |
| A6 | Engine | While disabled or paused for fullscreen, input release and "stop all rumble" run every frame (4 XInput calls × 125 Hz). | Wasted work, and the "turned off" rumble is cancelled immediately. |
| A7 | Haptics | Each pulse spawns a thread that stops *all* motors later, cutting newer pulses short. | Haptics feel inconsistent; threads are created on every key. |
| A8 | Keyboard | Split (dual-stick) mode: nothing sends `select_left`/`select_right`, the left stick still moves the mouse, and the right stick still scrolls. | Split mode cannot type. |
| A9 | Keyboard | The edit-tools layer has no entry key (the bottom row has two Ctrl keys). | Copy/Paste/Undo panel is unreachable. |
| A10 | Keyboard | Arabic Shift+T / Shift+G send presentation-form glyphs (U+FEF9/U+FEF7) instead of لإ / لأ. | Wrong characters reach apps and search. |
| A11 | Keyboard | Opening (L3, tray, text click) uses default size/position, ignoring scale and edge. | The keyboard jumps size on open. |
| A12 | Keyboard | A dragged/moved position is never saved, though Settings says "remember after restart". | Position resets on every restart. |
| A13 | Keyboard | R3 move mode moves the window twice per frame (two code paths). | Jerky, too-fast repositioning. |
| A14 | Keyboard | The keyboard always opens on the primary monitor. | Unusable on a second monitor. |
| A15 | Radial | The left stick moves the mouse while steering the radial menu; mouse clicks on a slice leave the menu open; the centre "cancel" does nothing. | Radial menu feels broken. |
| A16 | Hotkey | The toggle hotkey is registered only at startup; changing it in Settings has no effect until restart. | Saved shortcut does nothing. |
| A17 | Status | The runtime snapshot is written with write-through twice a second, forever. | Constant disk writes. |
| A18 | Tray | The tooltip says "ENABLED" at start and never follows controller toggles; the tray keyboard toggle bypasses the overlay mode. | Misleading status. |
| A19 | Startup | Disabling autostart leaves the elevated logon task in place; elevation relaunch drops quoting for paths with spaces. | App still starts at logon; `--config` with spaces breaks. |
| A20 | OSD | Volume OSD reads the level before the key changes it. | The shown volume is one step behind. |
| A21 | Sound | Only key clicks respect the "sounds" setting. | Snap sounds play with sounds off. |
| A22 | Logs | `debug.log` grows without limit. | Unbounded disk use. |
| A23 | Engine | Analog triggers bound to Ctrl/Shift have no hysteresis. | A trigger resting at the threshold chatters the modifier. |
| A24 | Docs | README advertised offline Whisper voice, an emoji browser, a `voice` feature, 131 tests and unmeasured RAM/latency numbers. | Users and builders are misled. |

### B. New features that complete the experience

| # | Feature | Why |
|---|---------|-----|
| B1 | Fullscreen exceptions (browsers, video players) keep desktop control in fullscreen. | Fullscreen YouTube currently locks the user out of the pointer. |
| B2 | Low-battery warning: OSD + gentle rumble once when a wireless pad drops to Low/Empty. | Avoid surprise disconnects. |
| B3 | Live tray: state-coloured icon drawn in code, tooltip with mode, profile, battery. | Status visible at a glance. |
| B4 | `--doctor` report: config, XInput, controllers, elevation, autostart, fullscreen state, profiles, binding conflicts. | Self-service troubleshooting. |
| B5 | Binding conflicts detected (duplicate input + layer) in Settings and `--doctor`. | Explains "this button does nothing". |
| B6 | More quick actions: snip, emoji panel, clipboard history, quick settings, notifications, File Explorer, zoom, browser back/forward, copy/paste/undo, next/previous profile. | Common desktop tasks one press away. |
| B7 | Stick roles in Settings (pointer/scroll/window drag/off) — enables left-handed swap. | Sticks were fixed labels. |
| B8 | Back-layer bindings can be added for any button that has none. | Only pre-existing rows were editable. |
| B9 | Unsaved-changes indicator; delete-profile confirmation. | Avoid accidental loss. |
| B10 | Personal word learning for predictions (local only, opt-out). | Suggestions improve with use. |
| B11 | Accurate on-keyboard controller cheat sheet. | Hints described wrong buttons. |

### C. Release hygiene

Version 1.3.0, CHANGELOG, release notes, README (EN/AR) rewritten to match the
code, headless Clippy added to `scripts/check.ps1`.

## Hardware acceptance still required

These changes are verified by unit tests and builds. Feel on real hardware —
split-keyboard ergonomics, haptic strength, trigger hysteresis, multi-monitor
placement, and fullscreen detection per game — still needs a controller run.

## Release bar

- A new user can complete setup and everyday desktop tasks without a physical
  mouse or keyboard.
- No disconnect, mode switch, or fullscreen transition leaves an injected key
  or mouse button held.
- Text reaches the intended app, and the keyboard never permanently steals
  focus or hides the active field.
- The app reports the same profile and state wherever the user sees them.
- All feature builds and tests pass; performance claims have a recorded method.

## Status after the v1.3.0 pass

Every item in tables A, B and C above is implemented and covered by tests,
except where a fix can only be judged on hardware. The settings window was
additionally redesigned (sidebar, cards, bilingual English/Arabic with RTL
layout) and the shortcut library grew to 86 actions so a controller can stand
in for the mouse and keyboard in everyday use.
