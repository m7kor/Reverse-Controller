# 🎮 Controller Companion — Native Rust Edition

[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-blue?logo=windows)](https://microsoft.com/windows)
[![License](https://img.shields.io/badge/License-MIT-green)](LICENSE)
[![Memory Footprint](https://img.shields.io/badge/RAM-~31%20MB-brightgreen)]()
[![Idle GPU](https://img.shields.io/badge/GPU%20Idle-0.00%25-brightgreen)]()

A high-performance, ultra-lightweight native **Rust** re-engineering of the Windows desktop controller utility **Controller Companion**. It seamlessly transforms your Xbox/XInput controller into a mouse and keyboard with Windows 11 Fluent Acrylic visuals, full native Arabic keyboard support, and 100% offline AI voice dictation.

---

## ✨ Features (المميزات الرئيسية)

- ⚡ **Ultra-Lightweight & Efficient (استهلاك فائق الخفة للموارد):**
  - **~31 MB RAM** in background idle (vs ~242 MB in legacy .NET).
  - **0.00% GPU utilization** when overlays are hidden using smart event-driven sleeping loops.
  - Zero background latency: wakes up instantly (**0 ms**) on demand.
- 🎯 **Pixel-Perfect Stick Physics & AltSnap:**
  - Independent deadzones, non-linear acceleration curves, and sub-pixel fractional leftovers.
  - Diagonal 8-way snapping and 4-way vertical scroll locking.
  - Sniper mode (LT button) for fine precision control.
- 🔄 **Universal Hardware Back+Start Chord (اختصار التشغيل والإيقاف الفوري):**
  - Pressing **Back + Start** at any time instantly toggles the controller between desktop mouse mode and native game mode.
  - Works **even while disabled**, with haptic rumble pulse and animated OSD feedback.
- 🎮 **Automatic Game / Fullscreen Detection:**
  - Auto-pauses input when exclusive or borderless fullscreen games are focused, leaving full controller access to the game.
- ⌨️ **Windows 11 Fluent 2 Translucent Keyboard:**
  - Beautiful Mica / acrylic translucent dark theme with sound effects (mechanical switch SFX).
  - **Full Arabic Layout:** Native bidirectional shaping, ligatures (`لا`), and full Harakat/Tashkeel support (`َ ِ ُ ً ٍ ٌ ّ ْ`).
  - Automatic smart repositioning to avoid covering the active cursor/caret.
  - Smart auto-open when clicking text fields with the controller.
- 🎙️ **100% Offline AI Voice Dictation (إملاء صوتي بالذكاء الاصطناعي أوفلاين):**
  - Local speech-to-text powered by Whisper GGML (zero internet connection needed, 100% private).
  - Automatic Voice Activity Detection (VAD) stops recording upon speech pauses.
  - Automatic model unloading after 75s of idle to reclaim ~120 MB RAM instantly.
- 🍕 **Quick 8-Way Radial Pie Menu:**
  - Fast thumbstick selector for common actions (Browser, Task View, Explorer, Media, Virtual Desktops).
- ⚙️ **Modern Tray & Settings UI:**
  - Left-click tray icon to toggle control on/off.
  - Double-click to open Settings.
  - Right-click for context menu.
  - Import existing `.amgp` profiles with automatic conversion.

---

## 🎮 Default Controller Bindings (التحكم الافتراضي)

### Base Layer (الطبقة الأساسية)

| Input | Action | Function |
| :--- | :--- | :--- |
| **Left Stick** | Mouse Pointer | Moves the mouse cursor smoothly with acceleration |
| **Right Stick** | Mouse Wheel | Vertical & Horizontal scrolling |
| **A** | Left Click | Primary mouse click |
| **X** | Right Click | Secondary context menu click |
| **Right Thumb (R3)** | Middle Click | Browser tab close / auto-scroll |
| **Left Thumb (L3)** | Virtual Keyboard | Toggles the on-screen acrylic keyboard |
| **B** | Backspace | Delete previous character |
| **Y** | Play / Pause | Media playback control |
| **LB / RB** | Prev / Next Track | Skip media tracks |
| **LT (Left Trigger)** | Ctrl / Sniper Mode | Precision slow-down cursor + Ctrl modifier |
| **RT (Right Trigger)**| Shift Modifier | Shift key hold |
| **Start** | Enter | Submit / Newline |
| **Guide (Xbox)** | Windows Key | Opens Windows Start Menu (Hold 2.5s to turn off controller) |
| **Back (View)** | Modifier Layer | Hold for secondary functions below |

### Back Modifier Layer (اضغط مع زر Back)

| Chord | Action | Function |
| :--- | :--- | :--- |
| **Back + Start** | **Toggle Controller** | **تشغيل / إيقاف البرنامج مع اهتزاز وإشعار OSD** |
| **Back + Left Shoulder** | Voice Dictation | يبدأ الإملاء الصوتي بالذكاء الاصطناعي (أوفلاين) |
| **Back + Right Thumb (R3)**| Radial Menu | يفتح القائمة الدائرية السريعة |
| **Back + Left Thumb (L3)** | Drag Lock | قفل السحب والإفلات بدون الاستمرار بالضغط |
| **Back + D-Pad Up / Down** | Volume Up / Down | رفع وخفض مستوى الصوت |
| **Back + D-Pad Left / Right**| Snap Window | محاذاة النافذة لليمين أو اليسار |
| **Back + B** | Escape | إغلاق النوافذ والقوائم |
| **Back + X** | Show Desktop | إظهار سطح المكتب (Win+D) |
| **Back + Guide** | Turn Off Pad | إطفاء يد التحكم اللاسلكية فوراً |

---

## 🚀 Getting Started (التشغيل السريع)

### Download Pre-built Release
Run `controller-companion.exe`. The app sits in your system tray:
- **Left click tray icon:** Toggles controller on/off.
- **Double click tray icon:** Opens Settings.
- **Press Back + Start on controller:** Toggles controller on/off from anywhere.

---

## 🛠️ Building From Source (طريقة البناء)

### Prerequisites
- Windows 10 or Windows 11 (x64)
- [Rust toolchain (stable MSVC)](https://rustup.rs/)

### Build Release Binary
```powershell
# Clone the repository
git clone https://github.com/your-username/ControllerCompanion-Rust.git
cd ControllerCompanion-Rust

# Build optimized release binary
cargo build --release --features "gui,voice"
```
The optimized executable will be generated at:
`target/release/controller-companion.exe`

### Run Tests
To verify all 103 unit tests:
```powershell
cargo test --features "gui,voice"
```

---

## ⚙️ Configuration & Customization

Settings are stored in JSON format at:
`%APPDATA%\ControllerCompanionRust\config.json`

Example configuration snippet:
```json
{
  "enabled": true,
  "disable_in_fullscreen": true,
  "autostart": false,
  "vk_auto_show": true,
  "toggle_hotkey": "Ctrl+K",
  "mouse": {
    "speed": 600.0,
    "acceleration": 800.0,
    "wheel_speed": 3600.0
  }
}
```

---

## 📊 Benchmark: Native Rust vs Legacy .NET

| Metric | Legacy .NET 4.5 | Controller Companion Rust |
| :--- | :---: | :---: |
| **Working Set (RAM)** | ~240 MB+ | **~31 MB (-87%)** |
| **Idle GPU Usage** | ~0.6% Continuous | **0.00% (Event Sleeping Loop)** |
| **Startup Time** | ~1.8s | **~0.08s (Instant)** |
| **Arabic Keyboard Support** | None / Garbled | **Full Reshaping & Ligatures** |
| **Offline Voice AI** | None | **Built-in Whisper GGML** |
| **Dependencies** | .NET Runtime, WPF | **Zero runtime dependencies (Single .exe)** |

---

## 📜 License

Distributed under the MIT License. See `LICENSE` for more information.
