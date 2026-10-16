# Controller Companion quality plan

## Product goal

Replace the physical mouse and keyboard for everyday Windows desktop use.
After launching the app, a user should be able to complete setup, navigate,
click, scroll, type in Arabic and English, and recover from interruptions using
only the controller. Movement should feel predictable, and the app should
always explain whether desktop control is active, paused, or unavailable.

## Baseline (16 October 2026)

- The GUI and voice test suite passes: 103 tests.
- The full-feature build checks successfully. The headless build initially
  failed because GUI-only modules were referenced without the `gui` feature;
  this has been fixed and the headless build now checks successfully.
- Settings previously put profiles, pointer tuning, keyboard preferences,
  controller tuning, and bindings in one long page. They now have separate
  sections, with an overview and quick controls.
- Saving from the settings window now reports failure and keeps the window
  open. Reset prepares default values for review before the user saves them.
- The settings window runs in a separate process. The background engine now
  publishes a short-lived snapshot for its active/paused state, connection,
  battery, and profile; the settings overview and tray use the same state
  classification. This still needs a desktop and controller acceptance run.
- Disconnect handling had a release-order bug: the hardware connection flag
  could clear before held inputs were released. Input release no longer depends
  on that flag, and a regression test covers the case.
- Settings now keep Save and recovery actions visible while scrolling, use
  larger click targets, and merge edits with settings changed by the background
  process. Live reload rejects incomplete config data instead of silently
  resetting the running engine.
- The pointer settings page now shows both sticks live, including each
  configured dead-zone ring, so calibration can be done with the controller.
- The controller's Back + Start recovery chord has a regression test covering
  disabled state, hold behavior, and the next deliberate press.
- `--status` is read-only and checks every XInput slot. The current host's
  status probe found no connected controller, so live hardware acceptance is
  still outstanding; synthetic controller tests cover recovery logic meanwhile.
- Published RAM, GPU, startup, and latency numbers in the README need repeatable
  measurement before they can be treated as release claims.

## Next milestones

1. **Observe the real experience.** Run the settings window and keyboard on a
   Windows desktop with a controller. Capture each core journey: connect,
   navigate, type, enter a game, return, disconnect, and recover. Record visual
   and functional issues with a clear reproduction path.
2. **Complete controller-only setup.** Verify that every settings action,
   including selecting fields, changing values, saving, and recovery, is
   possible using only the controller. Add dedicated focus navigation where
   pointer emulation alone is awkward, and make text entry available at every
   editable field.
3. **Calibrate movement.** Build on the live stick and dead-zone display with
   a short guided flow, a pointer practice area, and plain-language presets.
   Keep advanced curve controls available.
4. **Perfect typing.** Exercise Arabic and English, mixed-direction text,
   diacritics, shift, held keys, prediction, voice, focus preservation, and
   keyboard placement on different display scales and monitors.
5. **Finish the visual system.** Apply one set of typography, spacing, focus
   states, colors, and feedback across settings, keyboard, radial menu, and
   notifications. Check contrast and controller navigation at every step.
6. **Release with evidence.** Verify startup and exit, input release on every
   interruption, profile changes, configuration recovery, packaging, and
   resource claims. Update the README to match observed behavior.

## Release bar

- A new user can complete setup and everyday Windows desktop tasks without a
  physical mouse or keyboard after opening the app.
- No controller disconnect, mode switch, or fullscreen transition leaves an
  injected key or mouse button held.
- Text reaches the intended app, and the virtual keyboard never permanently
  steals focus or hides the active field.
- The app reports the same active profile and desktop-control state wherever
  the user sees them.
- All supported feature builds and relevant tests pass, and performance claims
  have a recorded measurement method.
