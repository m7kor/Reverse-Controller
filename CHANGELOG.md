# 📜 Changelog

All notable changes to the **Reverse Controller** (Controller Companion — Native Rust Edition) project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---
## [v1.3.2] — 2026-10-18

### 🚀 Highlights & Critical Bug Fixes
- **AltSnap Hook Installation & Drag Lifecycle Overhaul**:
  - **Fixed Low-Level Hook Installation Failure**: Resolved `ERROR_HOOK_NEEDS_HMOD` (Win32 error 1428) where passing a null module handle caused `SetWindowsHookExW(WH_MOUSE_LL)` to fail silently, completely disabling AltSnap on Windows. Hook now correctly registers with `GetModuleHandleW(std::ptr::null())`.
  - **Fixed Premature Mouse Move Cancellation**: Removed aggressive `GetAsyncKeyState(0x01)` check inside `WM_MOUSEMOVE` that caused false negatives on initial drag frames and terminated mouse drags before movement started. The drag lifecycle is now strictly governed by `WM_LBUTTONDOWN` -> `WM_MOUSEMOVE` -> `WM_LBUTTONUP` (and right click for resize).
  - **Comprehensive Modifier Key Detection**: Expanded modifier key checks to explicitly support generic and distinct physical keys (`VK_MENU`, `VK_LMENU`, `VK_RMENU` / AltGr for Alt; `VK_CONTROL`, `VK_LCONTROL`, `VK_RCONTROL` for Ctrl; `VK_SHIFT`, `VK_LSHIFT`, `VK_RSHIFT` for Shift; `VK_LWIN`, `VK_RWIN` for Win) across multilingual and Arabic keyboards.
  - **UWP / Modern App Dragging**: Removed overly restrictive `Windows.UI.Core.CoreWindow` filter so modern Windows 10/11 apps (Calculator, Settings, Terminal, modern Explorer tabs) can now be smoothly dragged, snapped, and resized.
  - **Escape Key Drag Cancellation**: Pressing `Esc` during an active window drag or resize immediately cancels the operation and snaps the window back to its original position.

---
## [v1.3.1] — 2026-10-18

### 🚀 Highlights & Bug Fixes
- **AltSnap Polish & Stutter-Free Window Dragging**:
  - Eliminated mouse cursor freezes, lag, and jitter during window movement.
  - Fixed axis-locking bug where windows could become locked vertically and only move horizontally.
  - Fixed unmaximize edge trapping: restoring maximized windows by dragging now includes an automatic grace margin, preventing immediate re-snapping or accidental re-maximizing.
  - Minimized Win32 GDI calls by caching monitor work areas, updating only when crossing monitor bounds.
  - Decoupled X and Y movement axes for zero-latency, buttery-smooth window movement.
  - Enhanced controller stick window drag to track true window bounds and pin cursor to the titlebar.
- **Start Menu & System Shell Overlay**:
  - The virtual keyboard now renders directly in front of (on top of) the Windows 11 Start Menu and system flyouts (`StartMenuExperienceHost.exe` & `SearchHost.exe`) at its natural position without dodging away or getting buried underneath.
  - Automatically manages Win32 Z-order hierarchy (`HWND_TOPMOST` and `HWND_TOP`) and shell foreground ownership so keystrokes type seamlessly into Start and Search.
- **Virtual Keyboard & Radial Menu Refinements**:
  - Eliminated unwanted yellow focus boxes/borders appearing on keys when opening keyboard or clicking from the Radial Menu (focus borders only activate on intentional D-pad arrow grid navigation).
  - Fixed Radial Menu 430x430 clipping when transitioning to the full virtual keyboard.
  - Radial Menu volume up/down slices stay open for smooth, continuous adjustment.
  - Cleaned up clipboard paste actions to avoid duplicated paste events.

---

## [v1.3.0] — 2026-10-18

### 🚀 Highlights
- **AltSnap native window management**: integrated full [AltSnap](https://github.com/RamonUnch/AltSnap) capabilities directly into the engine. Alt+Left Click anywhere moves windows with auto-maximize snapping; Alt+Right Click dynamically resizes windows based on cursor position relative to the 8 quadrant zones; Alt+Middle Click toggles maximize; Alt+Scroll cycles transparency. Magnetic edge snapping aligns windows to screen boundaries and taskbars. Toggles for Always-on-Top (pin), Roll Up / Shade, and Lower to back are supported via shortcuts and game controller mappings (analog stick `WindowResize` / `WindowDrag`). Fully customizable in Settings with dedicated English & Arabic UI.
- **Installer built in**: running the downloaded `.exe` offers a per-user install (no administrator rights) to `%LOCALAPPDATA%\Programs\Reverse Controller`, with a Start-menu shortcut and an entry in Windows Installed apps; a newer download updates it; `--uninstall` removes it and keeps settings. Declining is remembered (portable use). The executable now carries an app icon and version information.
- **Getting started tutorial**: opens on first launch; six steps (move, click, scroll, aim, type, quick menu) tick off as they are done on the controller.
- **Smart pointer**: the pointer eases off over links and buttons (detected from the hand cursor, without accessibility queries that would slow browsers) so small targets are easy to stop on; a full push still sweeps past. Adjustable on the Pointer page.
- **Button-mapping wizard**: teaches the app any non-Xbox controller's layout, button by button, saved per controller model.
- **Every kind of controller**: besides Xbox/XInput pads, PlayStation 5 (DualSense, DualSense Edge), PlayStation 4 (DualShock 4), Nintendo Switch Pro and generic USB/Bluetooth gamepads now work without extra drivers, through the Windows joystick API. Buttons map by position, PlayStation triggers stay analog, the PS/Home button acts as Guide. Settings, the tray tooltip and the diagnostics report name the controller in use. XInput pads are preferred, and a third-party Xbox-mode pad is never read twice.
- **Redesigned Settings window**: sidebar navigation, card layout, toggle switches and a dark theme that matches the keyboard. Eight pages (Home, Pointer, Controller, Keyboard, Buttons, Profiles, General, About), fully in **English and Arabic** with right-to-left layout and an Auto / English / العربية language choice. Unsaved-changes indicator, Ctrl+S, and a prompt before closing with unsaved edits.
- **Shortcut library**: 86 actions in seven groups for any button — browser tabs, copy/paste/save/find, snip, emoji panel, clipboard history, Run, Search, File Explorer, lock, Task Manager, magnifier, display modes and more. New actions: double click, cursor to next monitor, centre cursor on window, next/previous profile.
- **Buttons page**: stick roles with a one-click left-handed swap, add or remove second-layer (Back) shortcuts, conflict warnings, restore defaults.
- **Fullscreen exceptions**: browsers and video players keep desktop control in fullscreen; the list is editable.
- **Word learning**: words typed twice become keyboard suggestions, stored locally; can be turned off or cleared.
- **Keyboard toolbar**: shortcut buttons shrink to icons while word suggestions are shown.
- **Live tray icon**: a gamepad drawn in code, coloured by state, with mode, profile and battery in the tooltip; Diagnostics Report in the menu.
- **Diagnostics report** (`--doctor`, tray, Settings): settings, conflicts, profiles, XInput, controllers, elevation, monitors and fullscreen state, saved to `diagnostics.txt`.
- **Low-battery warning** once per connection for wireless pads.
- **Multi-monitor keyboard**: opens on the monitor you are working on and keeps a moved keyboard on its own monitor.

### 🐛 Fixes
- **0xc0000005 GPF exit crash fix**: resolved access violation popup on process termination by gracefully closing the WGPU/eframe graphics pipeline before exit, uninitializing STA COM threads cleanly, unhooking low-level hooks, and suppressing Windows GPF modal dialogs (`SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX`).
- **Physical keyboard lockout fix**: replaced low-level `WH_KEYBOARD_LL` hook with native Win32 `RegisterHotKey`, completely preventing system-wide physical keyboard freezes and latency.
- **Overlay focus stealing fix**: enforced `with_active(false)` and `WS_EX_NOACTIVATE` on virtual keyboard creation so off-screen overlays never steal input focus from active desktop apps.
- **Window title lookup & controller click bounds**: fixed overlay title mismatch (`CCVKbd` vs `Controller Companion Keyboard`) and bounded controller click tracking so physical mouse clicks properly dismiss the overlay.
- **Trigger threshold drift protection**: added lower bound safety and hysteresis to prevent analog trigger drift from latching held `Ctrl` or `Shift` modifier keys.
- **Strict fullscreen detection restored**: reverted overly broad `QUNS_APP`/`QUNS_BUSY` states and coordinate tolerance back to strict exclusive D3D (`QUNS_RUNNING_D3D_FULL_SCREEN`) and exact monitor dimensions, preventing windowed apps from pausing controller input.
- **Text selection & auto-show fix**: removed false-positive `is_cursor_ibeam()` checks and added mouse drag tracking so selecting/highlighting text across documents or web pages behaves like a regular mouse and no longer triggers the on-screen virtual keyboard.
- Quitting while an app-specific profile was active saved its bindings over your own; exit now writes back only on/off, a profile you chose from the controller, and the keyboard position.
- Auto-switching returned to *Default* instead of your chosen profile, switched when focusing the app's own windows, and stopped re-applying after a settings reload.
- `ActivateProfile`, `NextVirtualDesktop`, `PrevVirtualDesktop` and voice typing actions did nothing.
- While a fullscreen game was focused the engine zeroed every controller motor 125 times a second, cancelling the game's own rumble; idle states now release input once and leave the motors alone.
- Each haptic pulse spawned a thread that could cut newer pulses short; one watchdog now serves all pulses.
- Split keyboard mode could not type: the triggers now type the left/right key, both sticks move the cursors, and neither moves the pointer.
- The editing panel had no entry key (the bottom row had two Ctrl keys); the right Ctrl is now ✂ Edit.
- Arabic Shift+T / Shift+G sent presentation-form glyphs instead of لإ / لأ.
- The keyboard ignored size and edge preferences when opened from L3, the tray or a text click, never saved a moved position, moved twice per frame in R3 mode, and jumped back when switching language.
- Radial menu: the left stick also moved the pointer, clicking a slice left the menu open, the centre did not cancel, and choosing *Keyboard* with the controller closed the overlay.
- A changed toggle shortcut took effect only after a restart.
- The runtime status file was rewritten with write-through twice a second; it is now written on change plus a light heartbeat.
- Turning off *Start with Windows* left the elevated logon task in place; elevation relaunch broke paths containing spaces.
- Volume notifications showed the level from before the key press; menu sounds ignored the sound setting; `debug.log` grew without limit (now rotated at 1 MB).
- Analog triggers bound to Ctrl/Shift could chatter at the threshold (hysteresis added).
- Registry calls declared a 64-bit return value for a 32-bit `LONG`, so a success could be misread as an error.
- A hand-edited `{"WindowDrag": null}` binding failed to load.
- Third-party pads reporting an unknown battery showed "Empty" and could trigger a false low-battery warning.
- *Show controls when opened* was saved but never applied.
- README described features that are not in the app (offline Whisper voice, emoji browser, a `voice` build feature) and the wrong default buttons; both READMEs were rewritten from the code.

### 🛡️ Stability
- Clicking with A no longer runs a cross-process accessibility query on the input thread; text-box detection moved to a background thread, so an unresponsive window can never freeze the controller.
- Window dragging and showing the keyboard from other threads use asynchronous window calls, so a hung app or a busy overlay cannot stall input.
- The status snapshot uses cached connection and battery data instead of querying the controller every 250 ms under the engine lock (removes a periodic pointer stutter).
- Empty XInput slots are scanned at most twice a second while no controller is connected.
- Console output can no longer crash the app when its terminal is closed (release builds abort on panic).
- If the app ever crashes, the panic handler releases held modifiers, mouse buttons and rumble; startup also clears input left stuck by a previous crash.
- A disconnect while holding A in keyboard pointer mode no longer leaves the left mouse button down.
- Holding Guide to power the controller off no longer opens the Start menu.
- A released scroll stick no longer leaves a fractional tick that fires on the next touch.
- Startup registrations (`schtasks`) run in the background instead of delaying input.
- Starting the app again while it runs opens (or focuses) the Settings window instead of exiting silently; startup entries pass `--autostart` so sign-in launches stay quiet.
- A damaged `config.json` no longer stops the app from starting: the last good copy (`config.json.bak`) or the defaults are used, the damaged file is kept aside as `config.broken-<time>.json`, and a notification explains what happened.
- An input-loop watchdog releases held keys and buttons if the loop ever stops ticking for 2 seconds, and logs the stall.
- Unit tests no longer type or click into the focused window or write to the user's debug log.
- Emoji and voice-typing toolbar buttons removed; the keyboard's mixed Arabic labels (✂ تحرير) render correctly; edit-panel labels use one size; the radial menu no longer shows a grey square behind the circle.

### 🧪 Quality
- 181 tests in the full build and 135 in the headless build; strict Clippy is clean for both, and `scripts/check.ps1` now checks both.

---

## [v1.2.3] — 2026-10-17

### 🚀 Highlights
- **Gamepad Mouse Pointer Clicking via Button A**: Re-engineered controller input while the keyboard is visible so Button A acts as a native mouse left click when using the left stick to steer the pointer, allowing direct click-to-type on any key, toolbar button, or window with zero dropped inputs.
- **Dynamic D-Pad Grid Highlight**: The yellow selection box is completely hidden while using the mouse pointer and only appears when the user explicitly navigates with the D-pad arrow keys, seamlessly switching between mouse aiming and discrete grid typing.
- **Direct Shortcuts on Keyboard & Toolbar**: Added dedicated **Copy (`نسخ`)** and **Paste (`لصق`)** buttons directly to the bottom row of the keyboard layout, alongside a full suite of editing shortcuts in the top action toolbar (**Copy**, **Paste**, **Cut**, **Undo**, **Select All**).
- **Physical Mouse Clicking on Virtual Keyboard**: Completely overhauled virtual keyboard key interactions (`paint_key`) to seamlessly handle physical mouse and touch clicks with zero latency.
- **Inverted L1 and R1 Caret Movement**: Swapped shoulder bumper navigation (`LB` -> moves right, `RB` -> moves left) matching natural user thumb expectations.
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
