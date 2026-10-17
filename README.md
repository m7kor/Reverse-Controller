<div align="center">

# 🎮 Reverse Controller
### *Your game controller as a complete mouse and keyboard for Windows — native Rust, Arabic and English*

[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-DEA584?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D4?style=for-the-badge&logo=windows&logoColor=white)](https://microsoft.com/windows)
[![Release](https://img.shields.io/github/v/release/m7kor/Reverse-Controller?style=for-the-badge&color=2ea44f&logo=github)](https://github.com/m7kor/Reverse-Controller/releases/latest)
[![License](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)

**[🇸🇦 العربية](README.ar.md)** • **English** • **[📥 Download](https://github.com/m7kor/Reverse-Controller/releases/latest)** • **[📜 Changelog](CHANGELOG.md)**

</div>

> [!TIP]
> **No Steam · No internet · No telemetry.** Settings and learned words stay on your PC.

---

## Contents

| Section | What you'll find |
| :--- | :--- |
| [Install](#-install) | Download, install, uninstall |
| [Overview](#-overview) | What it does and who it is for |
| [Controller guide](#-controller-guide) | Every button in desktop, keyboard and shortcut modes |
| [On-screen keyboard](#%EF%B8%8F-on-screen-keyboard) | Typing in Arabic and English with a controller |
| [Settings](#%EF%B8%8F-settings) | The settings window, page by page |
| [Profiles](#-profiles) | Button sets per app, switched automatically |
| [Troubleshooting](#-troubleshooting) | The diagnostics report and common fixes |
| [Building](#%EF%B8%8F-building-from-source) | Toolchain, tests and command-line options |

---

## 📥 Install

1. Download `controller-companion.exe` from [Releases](https://github.com/m7kor/Reverse-Controller/releases/latest).
2. Run it. It offers to install for your user — no administrator rights needed: it goes to `%LOCALAPPDATA%\Programs\Reverse Controller`, appears in the Start menu, and can be removed from **Settings → Apps → Installed apps**.
3. Choose **No** to run it as a portable copy instead (you won't be asked again).
4. On the first start, Settings opens on **Getting started**: a one-minute tutorial that ticks off each step as you do it on the controller.

Running a newer download updates the installed copy. Command line: `--install`, `--uninstall [--quiet]`.

---

## 📖 Overview

**Reverse Controller** is an open-source, native Rust re-engineering of *Controller Companion* for Windows. It turns almost any game controller into a full replacement for the mouse and keyboard — no extra drivers:

| Controller | How it connects | Notes |
| :--- | :--- | :--- |
| Xbox Series / One / 360 and Xbox-mode pads | XInput | Vibration and battery level |
| PlayStation 5 — DualSense, DualSense Edge | Windows joystick API | Analog triggers, PS button = Guide |
| PlayStation 4 — DualShock 4 | Windows joystick API | Analog triggers, PS button = Guide |
| Nintendo Switch Pro | Windows joystick API | Buttons by position (bottom = A) |
| Other USB / Bluetooth gamepads | Windows joystick API | Standard DirectInput layout |

Buttons are mapped by position, so every binding works the same on every pad. When several are connected, an Xbox-type pad is used first.

What it does:

- 🖱️ **Pointer and scrolling** with dead zones, acceleration, a sustained-push cruise curve, LT *sniper* slow-down, optional AltSnap precision, and a **smart pointer** that eases off over links and buttons so small targets are easy to hit (a full push still sweeps past at full speed).
- ⌨️ **On-screen keyboard** for Arabic and English: D-pad or pointer typing, a split two-stick mode, word suggestions that learn your words, and an editing panel.
- 🪟 **Window and desktop control**: snap, maximise, minimise, close, task switcher assist, virtual desktops, window dragging with the right stick, radial quick menu.
- 📚 **A library of 80+ shortcuts** you can put on any button: browser tabs, copy/paste/save/find, screenshot, emoji panel, clipboard history, Run, Search, File Explorer, lock, Task Manager, magnifier, cursor-to-next-monitor and more.
- 🎮 **Games keep their controller**: desktop control pauses automatically in fullscreen games (browsers and video players stay controllable) and never touches the game's rumble.
- 🌐 **Bilingual app**: the whole settings window is available in English and Arabic (right-to-left), selectable at any time.

---

## 🎮 Controller guide

### 1. Desktop mode (default buttons)

| Button | Action |
| :--- | :--- |
| **Left stick** | Move the pointer |
| **Right stick** | Scroll (vertical and horizontal) |
| **A** | Left click (hold to drag) |
| **X** | Right click |
| **B** | Backspace |
| **Y** | Play / pause media |
| **D-pad** | Arrow keys (repeat while held) |
| **LB / RB** | Previous / next track |
| **LT** | Ctrl — squeezing it also slows the pointer (sniper mode) |
| **RT** | Shift — e.g. RT + D-pad selects text |
| **L3** | Open / close the on-screen keyboard |
| **R3** | Middle click |
| **Start** | Enter |
| **Back** | Hold for the second layer (below) |
| **Guide** | Start menu · hold 2.5 s to turn a wireless controller off |

Every button can be changed on the **Buttons** page of Settings.

### 2. Second layer (hold Back)

| Combination | Action |
| :--- | :--- |
| **Back + Start** | Turn desktop control on/off (always works, even while off) |
| **Back + B** | Escape |
| **Back + X** | Minimise all windows (Win+M) |
| **Back + D-pad ← / →** | Snap window left / right |
| **Back + D-pad ↑ / ↓** | Volume up / down (with on-screen meter) |
| **Back + LB** | Mute |
| **Back + RB** or **Back + RT** | Window switcher (Ctrl+Alt+Tab) |
| **Back + LT** | Previous track |
| **Back + L3** | Drag-lock (keeps the left button held) |
| **Back + R3** | Radial quick menu |
| **Back + right stick** | Move the window under the pointer |
| **Back + A / Back + Y** | Display: external only / PC screen only |
| **Back + Guide** | Turn the controller off |

Add a second-layer shortcut to any free button from the Buttons page.

### 3. Task switcher assist

While Alt+Tab or Task View is open: **D-pad / left stick / LB / RB** move between windows, **A** opens, **B** cancels, **X** closes the highlighted window.

### 4. Radial quick menu (Back + R3)

Steer with the left stick, press **A** (or click): Keyboard · Volume up · Task View · Volume down · Desktop · Paste · Close app · Screenshot. The centre cancels.

---

## ⌨️ On-screen keyboard

Opens with **L3**, from the tray, or automatically when you click a text box with **A**. It opens on the monitor you are working on, at the size and edge you chose, and remembers where you moved it.

| Button | While the keyboard is open |
| :--- | :--- |
| **D-pad** | Move between keys (**A** presses the highlighted key) |
| **Left stick + A** | Point at any key and click it |
| **Up from the top row** | Quick actions and word suggestions |
| **X / Y** | Backspace / Space (repeat while held) |
| **RT + X / RT + Y** | Delete word / accept the top suggestion |
| **LB / RB** | Move the text cursor left / right |
| **LT** | Numbers and symbols |
| **RT** | Shift (double-tap the Shift key for caps lock) |
| **Start / RT + Start** | Enter / new line without sending (Shift+Enter) |
| **Back** | Switch language (EN ⇄ ع) |
| **R3** | Move mode: place the keyboard with the right stick, R3 again to lock |
| **L3 + R3** | Split mode: each stick moves its own cursor, **LT/RT** type the left/right key |
| **B / L3** | Close |

**Typing aids**

- **Arabic layout** follows the Windows Arabic (101) keyboard, including Shift diacritics and the lam-alef keys (لا · لأ · لإ · لآ) sent as real letters.
- **English layout** mirrors whatever Windows layout is active (QWERTZ, AZERTY …).
- **✂ Edit panel**: copy, paste, cut, select all, undo, redo, delete word, Home/End, arrows.
- **Toolbar**: live preview, three suggestions, copy, paste, new line.
- **Word learning**: words you type twice become suggestions. Stored only in `learned_words.json` next to your settings; turn it off or clear it on the Keyboard page.

---

## ⚙️ Settings

Left-click the tray icon (or run `controller-companion.exe --settings`). The window has a page per topic and works fully in **English or العربية** — switch from the sidebar or the General page.

| Page | Highlights |
| :--- | :--- |
| **Home** | Live status, profile, battery, conflict warnings, and what every button does right now |
| **Pointer** | Presets (Precision / Balanced / Large display / Fast), speed, acceleration, sniper, AltSnap, stick curve, scrolling |
| **Getting started** | A guided tutorial: move, click, scroll, aim, type, quick menu — each step ticks off when you do it |
| **Controller** | Supported controllers, live button/trigger/stick monitor, **button-mapping wizard** for any non-Xbox pad, 4-second drift calibration, dead zones, trigger press point, vibration, low-battery warning |
| **Keyboard** | Position, size, remembered spot, caret avoidance, auto-open, split by default, sound effects, word learning |
| **Buttons** | Shortcut library for every button, stick roles with a one-click left-handed swap, add/remove second-layer shortcuts |
| **Profiles** | Activate, duplicate, delete (with confirmation), create from current buttons with program rules, import `.amgp` |
| **General** | Language, start with Windows, install/uninstall, fullscreen pause and its exception list, notifications, keyboard toggle shortcut, tutorial, diagnostics |

Edits are saved with **Save changes** (or Ctrl+S); closing with unsaved changes asks first. The background app picks up saved changes immediately, including a new toggle shortcut.

The **tray icon** is a gamepad that changes colour with the state (active, paused for a game, off, no controller); its tooltip shows the mode, profile and battery.

---

## 🗂 Profiles

A profile is a saved set of buttons. Give it program names (e.g. `vlc.exe, chrome.exe`) and it takes over while one of those programs is focused, then hands back to the profile you chose — automatic switches are never saved over your own choice. Profiles live in `%APPDATA%\ControllerCompanionRust\profiles`. AntiMicroX `.amgp` files can be imported from Settings or with `--convert`.

---

## 🩺 Troubleshooting

Run **Diagnostics report** from the tray menu or Settings → General (or `controller-companion.exe --doctor`). It checks the settings file, button conflicts, every profile, XInput and connected controllers, administrator rights, monitors and whether a fullscreen pause is active, then saves `diagnostics.txt` next to your settings.

| Symptom | Fix |
| :--- | :--- |
| Nothing moves | Press **Back + Start**; check the tray tooltip and the report |
| Paused in a browser or player | Add the program on General → *Keep control in these fullscreen programs* |
| Pointer drifts | Controller page → *Measure stick drift* → use the suggestion → Save |
| Can't click an admin window | Tray → *Restart as Administrator* |

---

## 🛠️ Building from source

Requirements: Windows 10/11 x64 and the [Rust MSVC toolchain](https://rustup.rs/).

```powershell
git clone https://github.com/m7kor/Reverse-Controller.git
cd Reverse-Controller
cargo build --release                 # target\release\controller-companion.exe
powershell -File scripts\check.ps1    # tests + strict Clippy, GUI and headless builds
```

`cargo build --no-default-features` builds a headless engine without the GUI.

**Command-line options**

| Option | Purpose |
| :--- | :--- |
| `--settings` | Open the settings window |
| `--config <path>` | Use another settings file |
| `--doctor` | Write the diagnostics report |
| `--status` | Print controller slots and battery |
| `--validate-config` | Check a settings file without starting |
| `--write-default` / `--print-default` | Write or print the default settings |
| `--convert <in.amgp> [out.json]` | Convert an AntiMicroX profile |
| `--no-elevate` | Don't ask for administrator rights at start |
| `--trace-pad`, `--inject-pad lx,ly,rx,ry[,buttons]` | Input diagnostics without/with a controller |

The app asks for administrator rights at start (so it can control elevated windows); use `--no-elevate` to skip.

Settings: `%APPDATA%\ControllerCompanionRust\config.json` · Debug log: `debug.log` in the same folder (rotated at 1 MB).

---

## 📜 License

MIT — see [LICENSE](LICENSE).
