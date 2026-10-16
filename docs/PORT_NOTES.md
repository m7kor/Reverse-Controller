# Port Notes — Controller Companion → Rust

This documents what was recovered from the original binaries, how REA was used,
what the Rust port implements, what was removed, and what is intentionally not
ported.

## 1. How the analysis was done

### Decompilation
`ilspycmd` (ILSpy CLI) was used to decompile `ControllerCompanion.exe` and
`ControllerCompanion.Lib.dll` (a .NET Framework 4.5.1, CIL/anyCPU assembly) into
C#. Output lives in `../decompiled/`. This recovered the exact per-frame logic:
`ControllerLoop`, `BindingSet`, `ButtonBindingEntry`, `MouseMovementBinding`,
`MouseScrollBinding`, `XInput` dead-zone math, `SendInputKeyPresser`,
`KeyRepeater`, `DefaultBindings`, and the settings model.

### REA (Reverse Engineer Anything)
[REA](https://github.com/morluto/rea) was installed (`npm i -g rea-agents`) and
used for evidence-backed managed analysis (its `rea-dotnet-static` provider runs
with **no** native engine required):

| REA command | Result |
|-------------|--------|
| `rea inspect-managed-artifact ControllerCompanion.exe` | PE/CLI identity: format `pe`, arch `x86`, runtime `.NET Framework 2.5/4.0.30319`, entry metadata-token, sha256 digest, section/CLI-header evidence. |
| `rea inspect-managed-native-boundaries ControllerCompanion.Lib.dll` | **Full P/Invoke inventory with evidence** — 125 `ImplMap` rows, module refs (`user32`, `kernel32`, `shell32`, `winmm`, `setupapi`, `xinput1_4/1_3/9_1_0`), entry points (`SendInput`, `SetWindowsHookEx`, `XInputGetState`, `XInputGetBatteryInformation`, `Shell_NotifyIconGetRect`, …). |
| `rea inspect-managed-members *.exe/.dll` | Type/member inventories (the Lib.dll exceeded a representation budget, so it fell back to the boundary scan). |

REA's extracted P/Invoke surface is mirrored 1:1 by `src/win32.rs` (raw
`extern "system"` + `#[repr(C)]` structs with a compile-time `INPUT == 40 bytes`
assertion), so the Rust FFI matches the original byte-for-byte on x64.

## 2. Feature inventory

### Implemented & verified in Rust
- **XInput reading** — dynamic load (`xinput1_4` → `xinput1_3` → `xinput9_1_0`),
  4 slots, radial **dead zones** (L 7849 / R 8689, exact original formula),
  battery info.
- **Mouse injection** — relative movement with **speed + acceleration +
  threshold + leftover-pixel accumulation** (port of `MouseMovementBinding`),
  buttons, wheel scroll (port of `MouseScrollBinding`).
- **Keyboard injection** — VK + `MapVirtualKeyW` scan codes, `EXTENDEDKEY`/
  `SCANCODE` flags (port of `SendInputKeyPresser`), OS-driven **auto-repeat**
  timing via `SystemParametersInfoW` (port of `KeyRepeater`), Unicode input for
  the OSK.
- **Default binding set** — exact port of `GetMouseModeBindings()` with the
  **`Back` modifier layer** (second-function row), double-buffered modifier
  state, and the original per-frame edge/dedup ordering.
- **Commands** — Win+M, Ctrl+Alt+Tab, `displayswitch.exe` (/external, /internal,
  /extend, /clone), controller power-off (best-effort).
- **Media/volume keys.**
- **Profiles** — JSON config save + **live reload**; default profile emitted via
  `--write-default` / `--print-default`.
- **Tray icon** — native `Shell_NotifyIconW` context menu: Enable/Disable,
  Settings, Reload, **Virtual Keyboard**, **Quit** (with a force-exit watchdog so
  Quit reliably ends the process).
- **Settings window** — egui/eframe: all mouse/controller tunables, enable,
  fullscreen-disable, autostart, live status/battery, binding overview.
- **Fullscreen auto-disable** — borderless-fullscreen foreground heuristic
  (port of `FullScreenLoopDisabler`).
- **Autostart** — HKCU `…\Run` registry entry (port of the non-Steam startup
  installer).
- **On-screen virtual keyboard** — new, tidy bottom-centre overlay (see §3).

### Removed (per the user's request) — all Steam integration
| Feature | Original symbols |
|---------|------------------|
| Steam Workshop profile sharing (download/upload/cache) | `SteamAPI`, `SteamHelper`, `ISteamWorkshopAPI`, `ProfileWorkshopUploader`, `WorkshopProfileProvider`, `WorkshopItemCache`, `WorkshopImageGenerator`, `WorkshopProfileApprovalPersister` |
| Steam Big Picture / client shortcuts | `steam://open/bigpicture`, `steam://open/main`, `steam://open/friends`, `SteamBigPicture`, `SteamOpen`, `SteamFriends` |
| Steam launch/overlay + uninstall hook | `steam_appid.txt` (367670), `SteamAppInstaller`, `uninstall.vdf` |
| Steam-aware Guide behaviour | `FileAssociation.IsAssociated("steam")` in `GetMouseModeBindings` |
| Steam settings | `SteamSettings` |

> The program does **not** require Steam to run; these were optional, Steam-tied
> extras. The port is fully standalone. In the default bindings, `Guide` now
> always opens the Windows Start menu (the Big Picture branch is gone).

### Not ported (documented, honest boundaries)
- **Chatpad (Xbox chatpad over the wireless receiver)** — requires the
  `libusb-win32` filter-driver stack (`LibUsbBin/*`, `install-filter.exe`,
  kernel filter) and a custom USB packet protocol. Porting is possible with
  `rusb`, but cannot be verified without the hardware/driver; excluded from this
  build to keep it honest.
- **Ease of Access registration / secure desktop** — `EaseOfAccessInstaller`,
  `AssistiveTechnologyRegistration`, secure-desktop profile sources.
- **Installers and uninstall flow** — the original installer is not ported.
  The current app has its own scheduled-task startup and single-instance guard.
- **x360ce emulator** installer/downloader and **update checker**. The current
  app has process-specific profile switching, an on-screen help overlay, and
  notifications beyond the tray.

## 3. The virtual keyboard (new)

The original *spiral* keyboard (`SpiralKeyboard`, `VirtualKeyboard*`) was not
rebuilt verbatim. It was replaced by a **modern, tidy grid overlay**
(`src/vkeyboard.rs`):

- **English + Arabic** layouts (data-driven rows), Unicode injection so Arabic
  types correctly in any app.
- Pinned **bottom-centre**, topmost, **no-activate** (`WS_EX_NOACTIVATE` via
  `enforce_overlay_style`) so it never steals focus from the target app.
- **Optional auto-show** on a detected text caret (`GetGUIThreadInfo`
  heuristic) — `vk_auto_show`, default off; toggle manually with **LeftThumb**
  or the tray menu.
- Shift latch, Space/Enter/Backspace/Tab/arrows/Esc, close button.

## 4. Known limitations

- **Guide button** and dedicated power-off use XInput's ordinal-only exports
  (`GetStateEx #100`, `PowerOff #103`) when the installed XInput DLL exposes
  them. They remain unavailable on DLLs without those exports.
- **Settings** run in a separate process to avoid an event-loop conflict with
  the virtual keyboard. A short-lived runtime snapshot shares the background
  engine's connection, mode, battery, and active profile with that window.

## 5. Build / run

```powershell
cargo build --release                     # with GUI + virtual keyboard
cargo build --release --no-default-features   # headless (no egui), tray + engine only
```

See `README.md` for CLI flags and the config file.
