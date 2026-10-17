# 📜 Changelog

All notable changes to the **Reverse Controller** (Controller Companion — Native Rust Edition) project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [v1.2.3] — 2026-10-17

### 🚀 Highlights
- **Physical Mouse Clicking on Virtual Keyboard**: Completely overhauled virtual keyboard key interactions (`paint_key`) to seamlessly handle mouse clicks with zero latency, eliminating dropped clicks on non-activating overlay windows while preserving instantaneous controller navigation and hold-to-repeat.
- **Inverted L1 and R1 Caret Movement**: Swapped shoulder bumper navigation (`B_LEFT_SHOULDER` -> moves right, `B_RIGHT_SHOULDER` -> moves left) matching user expectations for natural cursor motion.
- **Dedicated Shift+Enter Key on Keyboard**: Replaced duplicate bottom-row Enter with a dedicated `Shift+Enter` (`⇧↵`) key, allowing users to insert newlines in messaging applications (Discord, WhatsApp, Telegram, ChatGPT) without triggering accidental sends.
- **Gamepad Shift+Enter Shortcut (`RT + Start`)**: Introduced controller shortcut for `Shift+Enter`: holding `RT` (physical Shift trigger) while pressing `Start` (Enter) instantly sends `Shift+Enter` anywhere.
- **Foreground Target Sync on Text Click**: Added `platform::push_foreground()` to text click detection so virtual keyboard keystrokes and shortcuts always target the focused text window.

---

## [v1.2.2] — 2026-10-16

### 🚀 Highlights
- **OSK Text Box Flash & Dismissal Glitch Resolved**: Eliminated single-click dismissal when clicking into text fields and search inputs.
- **R3 Analog Keyboard Repositioning**: Click Right Stick (R3) to enter move mode, reposition the keyboard anywhere across monitors using the right stick, and press R3 again to lock.
- **Offline AI Voice Dictation Overhaul**: Upgraded local Whisper STT engine with Soft AGC, DC offset filter, greedy deterministic decoding (`temperature = 0.0`), expanded VAD timeout, and vocabulary biasing.
- **Dedicated Voice & AI Settings Page**: Live microphone VU meter, Whisper model diagnostics, language picker, VAD pause duration slider, and custom vocabulary editor.
- **Expanded Natural Voice Macros**: Natural punctuation and command triggers including "ارسل" (Enter), "سطر جديد" (newline), "فاصلة" (،), "نقطتان" (:), "بين قوسين" (()), and more.
- **Native Classified Emoji Picker & Edit Layer**: 100% offline emoji browser and one-tap editing actions (Copy, Paste, Cut, Undo, Redo, Select All).

### 🛠️ Changes & Improvements
- Added `clear_close_and_hide()` in `vknav.rs` and `vkeyboard.rs` to purge lingering signals on window surface.
- Extended external click immunity to 800ms to guarantee single-click focus into text controls.
- Added `normalize_audio()` in `voice.rs` with Soft AGC (up to 12.0× dynamic gain) and DC offset removal.
- Configured Whisper parameters with deterministic decoding, language domain priors, and single segment isolation.
- Added `VoiceSettings` struct in `config.rs` with JSON serialization, deserialization, and strict validation.
- Enhanced `process_voice_macros()` with macro enablement toggle and expanded Arabic/English punctuation replacements.
- Added bilingual repositioning guidance banner in `vkeyboard.rs`.
- Total unit tests increased to 131 with 100% pass rate and 0 Clippy warnings.

---

## [v1.2.1] — 2026-10-15

### 🚀 Highlights
- **Arabic Text Reshaping & BiDi**: Added full right-to-left layout shaping, ligature joining, and diacritics (tashkeel).
- **Taskbar Restart Resilience**: Registered `TaskbarCreated` Win32 message to automatically restore the notification tray icon if `explorer.exe` restarts.
- **Multi-Monitor Display Bounds**: Fixed virtual keyboard positioning to properly respect virtual desktop coordinates and per-monitor DPI.

---

## [v1.2.0] — 2026-10-14

### 🚀 Highlights
- **Offline Voice Typing**: Integrated `whisper-rs` with CPAL capture for 100% private CPU-based speech-to-text without Windows Voice Typing (Win+H) or cloud services.
- **Dual-Stick Split Keyboard**: Added Steam Deck-style split mode (L3+R3) allowing two-thumb simultaneous typing.
- **Custom Shoulder Triggers**: L1/R1 for smooth caret navigation, L2 for symbols, R2 for capitalization and Arabic diacritics.

---

## [v1.1.0] — 2026-10-13

### 🚀 Highlights
- **Pure Native Rust Core**: Complete rewrite of Controller Companion in Rust 2021 edition.
- **125 Hz Real-Time Engine**: Sub-pixel mouse physics with configurable curves, deadzones, and acceleration.
- **Zero Steam Dependency**: Completely standalone, portable binary without external runtimes.
- **AltSnap Window Dragging**: Smooth window snapping and movement using gamepad thumbsticks.
