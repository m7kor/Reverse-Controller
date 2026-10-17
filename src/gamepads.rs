//! Controllers that are not XInput devices: PlayStation (DualShock 4,
//! DualSense, DualSense Edge), Nintendo Switch Pro and generic USB/Bluetooth
//! gamepads.
//!
//! They are read through the Windows joystick API (`winmm`), which every
//! DirectInput-compatible game controller exposes without extra drivers, and
//! translated into an `XINPUT_GAMEPAD` so the engine, bindings and keyboard
//! work exactly as with an Xbox pad. Buttons are mapped by *position*: the
//! bottom face button is always A, the right one B, and so on.
//!
//! XInput devices also appear in this API, so Microsoft-made devices are
//! skipped here; the engine only uses this path while no XInput pad is
//! connected.

use crate::win32::XINPUT_GAMEPAD;
use serde::{Deserialize, Serialize};

#[repr(C)]
struct JOYCAPSW {
    w_mid: u16,
    w_pid: u16,
    sz_pname: [u16; 32],
    w_xmin: u32,
    w_xmax: u32,
    w_ymin: u32,
    w_ymax: u32,
    w_zmin: u32,
    w_zmax: u32,
    w_num_buttons: u32,
    w_period_min: u32,
    w_period_max: u32,
    w_rmin: u32,
    w_rmax: u32,
    w_umin: u32,
    w_umax: u32,
    w_vmin: u32,
    w_vmax: u32,
    w_caps: u32,
    w_max_axes: u32,
    w_num_axes: u32,
    w_max_buttons: u32,
    sz_reg_key: [u16; 32],
    sz_oem_vxd: [u16; 260],
}

#[repr(C)]
#[derive(Default)]
struct JOYINFOEX {
    dw_size: u32,
    dw_flags: u32,
    dw_xpos: u32,
    dw_ypos: u32,
    dw_zpos: u32,
    dw_rpos: u32,
    dw_upos: u32,
    dw_vpos: u32,
    dw_buttons: u32,
    dw_button_number: u32,
    dw_pov: u32,
    dw_reserved1: u32,
    dw_reserved2: u32,
}

// The structure sizes are part of the API contract; a wrong layout would make
// every call fail (or read garbage), so check them at compile time.
const _: () = assert!(std::mem::size_of::<JOYCAPSW>() == 728);
const _: () = assert!(std::mem::size_of::<JOYINFOEX>() == 52);

#[link(name = "winmm")]
extern "system" {
    fn joyGetNumDevs() -> u32;
    fn joyGetDevCapsW(id: usize, caps: *mut JOYCAPSW, size: u32) -> u32;
    fn joyGetPosEx(id: u32, info: *mut JOYINFOEX) -> u32;
}

const JOYERR_NOERROR: u32 = 0;
const JOY_RETURNALL: u32 = 0xFF;
const JOY_POVCENTERED: u32 = 0xFFFF;
/// winmm addresses at most 16 joysticks.
const MAX_JOYSTICKS: u32 = 16;
/// How often to look for a newly plugged-in controller while none is in use.
const RESCAN_SECS: f64 = 2.0;

const VENDOR_MICROSOFT: u16 = 0x045E;
const VENDOR_SONY: u16 = 0x054C;
const VENDOR_NINTENDO: u16 = 0x057E;

/// One axis of the winmm joystick state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
    R,
    U,
    V,
}

/// Which family a controller belongs to (for names and button layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    PlayStation4,
    PlayStation5,
    SwitchPro,
    Generic,
}

impl Family {
    pub fn from_ids(vendor: u16, product: u16) -> Family {
        match (vendor, product) {
            (VENDOR_SONY, 0x0CE6 | 0x0DF2) => Family::PlayStation5,
            (VENDOR_SONY, _) => Family::PlayStation4,
            (VENDOR_NINTENDO, 0x2009) => Family::SwitchPro,
            _ => Family::Generic,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Family::PlayStation4 => "PlayStation 4 (DualShock 4)",
            Family::PlayStation5 => "PlayStation 5 (DualSense)",
            Family::SwitchPro => "Nintendo Switch Pro",
            Family::Generic => "Gamepad",
        }
    }
}

/// Where each XInput button comes from on a winmm controller. Button numbers
/// are 1-based, as controller test panels show them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    /// Buttons in the A, B, X, Y positions (bottom, right, left, top).
    pub face: [u8; 4],
    pub lb: u8,
    pub rb: u8,
    pub lt: u8,
    pub rt: u8,
    pub back: u8,
    pub start: u8,
    pub l3: u8,
    pub r3: u8,
    pub guide: Option<u8>,
    pub right_stick: (Axis, Axis),
    /// Analog trigger axes, when the controller reports them.
    pub triggers: Option<(Axis, Axis)>,
}

impl Layout {
    /// Every button number is one a controller can report (1..=32).
    pub fn is_valid(&self) -> bool {
        let ok = |n: u8| (1..=32).contains(&n);
        self.face.iter().all(|n| ok(*n))
            && [self.lb, self.rb, self.lt, self.rt, self.back, self.start, self.l3, self.r3].iter().all(|n| ok(*n))
            && self.guide.is_none_or(ok)
    }

    pub fn for_family(family: Family) -> Layout {
        match family {
            // Square 1, Cross 2, Circle 3, Triangle 4, L1 5, R1 6, L2 7,
            // R2 8, Share/Create 9, Options 10, L3 11, R3 12, PS 13.
            Family::PlayStation4 | Family::PlayStation5 => Layout {
                face: [2, 3, 1, 4],
                lb: 5,
                rb: 6,
                lt: 7,
                rt: 8,
                back: 9,
                start: 10,
                l3: 11,
                r3: 12,
                guide: Some(13),
                right_stick: (Axis::Z, Axis::R),
                triggers: Some((Axis::U, Axis::V)),
            },
            // B 1 (bottom), A 2 (right), Y 3 (left), X 4 (top), L 5, R 6,
            // ZL 7, ZR 8, Minus 9, Plus 10, LS 11, RS 12, Home 13.
            Family::SwitchPro => Layout {
                face: [1, 2, 3, 4],
                lb: 5,
                rb: 6,
                lt: 7,
                rt: 8,
                back: 9,
                start: 10,
                l3: 11,
                r3: 12,
                guide: Some(13),
                right_stick: (Axis::Z, Axis::R),
                triggers: None,
            },
            // The common DirectInput layout (Logitech "D" mode and most
            // generic pads): X 1, A 2, B 3, Y 4, LB 5, RB 6, LT 7, RT 8,
            // Back 9, Start 10, L3 11, R3 12.
            Family::Generic => Layout {
                face: [2, 3, 1, 4],
                lb: 5,
                rb: 6,
                lt: 7,
                rt: 8,
                back: 9,
                start: 10,
                l3: 11,
                r3: 12,
                guide: Some(13),
                right_stick: (Axis::Z, Axis::R),
                triggers: None,
            },
        }
    }
}

/// Raw winmm state, kept separate from the FFI struct so it can be tested.
#[derive(Clone, Copy, Debug, Default)]
pub struct RawState {
    pub axes: [u32; 6], // X Y Z R U V
    pub buttons: u32,
    pub pov: u32,
}

/// Axis ranges reported by the device.
#[derive(Clone, Copy, Debug)]
pub struct Ranges {
    pub min: [u32; 6],
    pub max: [u32; 6],
}

fn axis_index(a: Axis) -> usize {
    match a {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
        Axis::R => 3,
        Axis::U => 4,
        Axis::V => 5,
    }
}

/// Map a raw axis value onto -32768..32767 (XInput stick units).
fn stick_value(raw: &RawState, ranges: &Ranges, a: Axis) -> i16 {
    let i = axis_index(a);
    let (min, max) = (ranges.min[i] as f64, ranges.max[i] as f64);
    if max <= min {
        return 0;
    }
    let t = ((raw.axes[i] as f64 - min) / (max - min)).clamp(0.0, 1.0);
    (t * 65535.0 - 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

/// Map a raw axis value onto 0..255 (XInput trigger units).
fn trigger_value(raw: &RawState, ranges: &Ranges, a: Axis) -> u8 {
    let i = axis_index(a);
    let (min, max) = (ranges.min[i] as f64, ranges.max[i] as f64);
    if max <= min {
        return 0;
    }
    (((raw.axes[i] as f64 - min) / (max - min)).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Translate one winmm reading into the XInput gamepad the engine expects.
pub fn to_gamepad(raw: &RawState, ranges: &Ranges, layout: &Layout) -> XINPUT_GAMEPAD {
    let pressed = |n: u8| (1..=32).contains(&n) && raw.buttons & (1 << (n - 1)) != 0;
    let mut b: u16 = 0;
    for (n, bit) in [
        (layout.face[0], 0x1000u16),
        (layout.face[1], 0x2000),
        (layout.face[2], 0x4000),
        (layout.face[3], 0x8000),
        (layout.lb, 0x0100),
        (layout.rb, 0x0200),
        (layout.back, 0x0020),
        (layout.start, 0x0010),
        (layout.l3, 0x0040),
        (layout.r3, 0x0080),
    ] {
        if pressed(n) {
            b |= bit;
        }
    }
    if layout.guide.is_some_and(pressed) {
        b |= 0x0400;
    }
    // Hat switch: hundredths of a degree clockwise from up, 0xFFFF = centred.
    if raw.pov != JOY_POVCENTERED && raw.pov <= 36000 {
        let deg = raw.pov / 100;
        if deg >= 315 || deg <= 45 {
            b |= 0x0001; // up
        }
        if (45..=135).contains(&deg) {
            b |= 0x0008; // right
        }
        if (135..=225).contains(&deg) {
            b |= 0x0002; // down
        }
        if (225..=315).contains(&deg) {
            b |= 0x0004; // left
        }
    }
    let (lt, rt) = match layout.triggers {
        Some((l, r)) => (trigger_value(raw, ranges, l), trigger_value(raw, ranges, r)),
        None => (
            if pressed(layout.lt) { 255 } else { 0 },
            if pressed(layout.rt) { 255 } else { 0 },
        ),
    };
    // winmm's Y axes grow downwards; XInput's grow upwards.
    let flip = |v: i16| v.checked_neg().unwrap_or(i16::MAX);
    XINPUT_GAMEPAD {
        wButtons: b,
        bLeftTrigger: lt,
        bRightTrigger: rt,
        sThumbLX: stick_value(raw, ranges, Axis::X),
        sThumbLY: flip(stick_value(raw, ranges, Axis::Y)),
        sThumbRX: stick_value(raw, ranges, layout.right_stick.0),
        sThumbRY: flip(stick_value(raw, ranges, layout.right_stick.1)),
    }
}

/// A connected non-XInput controller.
#[derive(Clone, Debug)]
pub struct Device {
    pub id: u32,
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    pub family: Family,
    ranges: Ranges,
    layout: Layout,
}

impl Device {
    /// What to call this controller in the UI.
    pub fn display_name(&self) -> String {
        display_name(self.family, &self.name, self.vendor, self.product)
    }
}

/// Windows names many pads after its own driver ("Microsoft PC-joystick
/// driver"), which tells the user nothing; show the USB ids instead.
fn display_name(family: Family, name: &str, vendor: u16, product: u16) -> String {
    let generic = name.is_empty() || name.to_ascii_lowercase().contains("pc-joystick");
    match family {
        Family::Generic if generic => format!("Gamepad ({vendor:04X}:{product:04X})"),
        Family::Generic => name.to_string(),
        family => family.label().to_string(),
    }
}

/// Read one joystick's raw state (the mapping wizard watches this).
pub fn read_raw(id: u32) -> Option<RawState> {
    let mut info = JOYINFOEX {
        dw_size: std::mem::size_of::<JOYINFOEX>() as u32,
        dw_flags: JOY_RETURNALL,
        ..Default::default()
    };
    if unsafe { joyGetPosEx(id, &mut info) } != JOYERR_NOERROR {
        return None;
    }
    Some(RawState {
        axes: [info.dw_xpos, info.dw_ypos, info.dw_zpos, info.dw_rpos, info.dw_upos, info.dw_vpos],
        buttons: info.dw_buttons,
        pov: info.dw_pov,
    })
}

fn read_device(id: u32) -> Option<Device> {
    let mut caps: JOYCAPSW = unsafe { std::mem::zeroed() };
    if unsafe { joyGetDevCapsW(id as usize, &mut caps, std::mem::size_of::<JOYCAPSW>() as u32) } != JOYERR_NOERROR {
        return None;
    }
    // XInput devices are read through XInput, never twice.
    if caps.w_mid == VENDOR_MICROSOFT {
        return None;
    }
    // Present in the registry is not plugged in: it must answer a read.
    read_raw(id)?;
    let len = caps.sz_pname.iter().position(|&c| c == 0).unwrap_or(caps.sz_pname.len());
    let name = String::from_utf16_lossy(&caps.sz_pname[..len]).trim().to_string();
    if name.to_ascii_lowercase().contains("xinput") {
        return None;
    }
    let family = Family::from_ids(caps.w_mid, caps.w_pid);
    Some(Device {
        id,
        name,
        vendor: caps.w_mid,
        product: caps.w_pid,
        family,
        ranges: Ranges {
            min: [caps.w_xmin, caps.w_ymin, caps.w_zmin, caps.w_rmin, caps.w_umin, caps.w_vmin],
            max: [caps.w_xmax, caps.w_ymax, caps.w_zmax, caps.w_rmax, caps.w_umax, caps.w_vmax],
        },
        layout: Layout::for_family(family),
    })
}

/// Every connected non-XInput controller (diagnostics and settings).
pub fn list_devices() -> Vec<Device> {
    let n = unsafe { joyGetNumDevs() }.min(MAX_JOYSTICKS);
    (0..n).filter_map(read_device).collect()
}

/// One entry per controller model: some pads show up as several joysticks
/// (one per USB interface), and a layout is learned per model anyway.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub fn list_models() -> Vec<Device> {
    let mut seen = std::collections::HashSet::new();
    list_devices().into_iter().filter(|d| seen.insert((d.vendor, d.product))).collect()
}

/// Polls the first connected non-XInput controller.
///
/// Probing unplugged joystick ids is slow, so the device list is only
/// rescanned every couple of seconds while nothing is in use; a controller in
/// use costs one read per frame.
#[derive(Default)]
pub struct GenericPads {
    active: Option<Device>,
    next_scan: f64,
    /// Layouts the user taught the app, by (vendor, product).
    overrides: Vec<(u16, u16, Layout)>,
}

impl GenericPads {
    /// Read the active controller, finding one first if needed.
    pub fn poll(&mut self, now: f64) -> Option<XINPUT_GAMEPAD> {
        if let Some(dev) = &self.active {
            match read_raw(dev.id) {
                Some(raw) => return Some(to_gamepad(&raw, &dev.ranges, &dev.layout)),
                None => {
                    crate::platform::log_debug(&format!("[gamepads] {} disconnected", dev.display_name()));
                    self.active = None;
                    self.next_scan = now + 0.5;
                    return None;
                }
            }
        }
        if now < self.next_scan {
            return None;
        }
        self.next_scan = now + RESCAN_SECS;
        let mut dev = list_devices().into_iter().next()?;
        if let Some(layout) = self.override_for(dev.vendor, dev.product) {
            dev.layout = layout;
        }
        crate::platform::log_debug(&format!(
            "[gamepads] using {} (vendor {:04X}, product {:04X}, joystick {})",
            dev.display_name(),
            dev.vendor,
            dev.product,
            dev.id
        ));
        let raw = read_raw(dev.id)?;
        let gp = to_gamepad(&raw, &dev.ranges, &dev.layout);
        self.active = Some(dev);
        Some(gp)
    }

    /// Use these learned layouts (from the settings) from now on.
    pub fn set_overrides(&mut self, overrides: Vec<(u16, u16, Layout)>) {
        self.overrides = overrides;
        if let Some((vendor, product)) = self.active.as_ref().map(|d| (d.vendor, d.product)) {
            let layout = self
                .override_for(vendor, product)
                .unwrap_or_else(|| Layout::for_family(Family::from_ids(vendor, product)));
            if let Some(dev) = self.active.as_mut() {
                dev.layout = layout;
            }
        }
    }

    fn override_for(&self, vendor: u16, product: u16) -> Option<Layout> {
        self.overrides
            .iter()
            .find(|(v, p, l)| *v == vendor && *p == product && l.is_valid())
            .map(|(_, _, l)| *l)
    }

    /// Is a non-XInput controller in use?
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Name of the controller in use, if any.
    pub fn active_name(&self) -> Option<String> {
        self.active.as_ref().map(Device::display_name)
    }

    /// Forget the active controller (an XInput pad took over).
    pub fn release(&mut self) {
        self.active = None;
    }
}


// ---------------------------------------------------------------------------
// Mapping wizard: teach the app an unknown controller's layout.
// ---------------------------------------------------------------------------

/// One thing the wizard asks the user to press or push.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardStep {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Lt,
    Rt,
    Back,
    Start,
    L3,
    R3,
    Guide,
    RightStickRight,
    RightStickDown,
}

#[cfg_attr(not(feature = "gui"), allow(dead_code))] // the wizard UI lives in the settings window
impl WizardStep {
    pub const ALL: [WizardStep; 15] = [
        WizardStep::A,
        WizardStep::B,
        WizardStep::X,
        WizardStep::Y,
        WizardStep::Lb,
        WizardStep::Rb,
        WizardStep::Lt,
        WizardStep::Rt,
        WizardStep::Back,
        WizardStep::Start,
        WizardStep::L3,
        WizardStep::R3,
        WizardStep::Guide,
        WizardStep::RightStickRight,
        WizardStep::RightStickDown,
    ];

    /// The instruction, in English and Arabic.
    pub fn prompt(self) -> (&'static str, &'static str) {
        match self {
            WizardStep::A => ("Press the BOTTOM face button (A · Cross · Nintendo B)", "اضغط زر الوجه السفلي (A · ✕ · B في نينتندو)"),
            WizardStep::B => ("Press the RIGHT face button (B · Circle · Nintendo A)", "اضغط زر الوجه الأيمن (B · ◯ · A في نينتندو)"),
            WizardStep::X => ("Press the LEFT face button (X · Square · Nintendo Y)", "اضغط زر الوجه الأيسر (X · ▢ · Y في نينتندو)"),
            WizardStep::Y => ("Press the TOP face button (Y · Triangle · Nintendo X)", "اضغط زر الوجه العلوي (Y · △ · X في نينتندو)"),
            WizardStep::Lb => ("Press the left bumper (LB · L1 · L)", "اضغط زر الكتف الأيسر (LB · L1 · L)"),
            WizardStep::Rb => ("Press the right bumper (RB · R1 · R)", "اضغط زر الكتف الأيمن (RB · R1 · R)"),
            WizardStep::Lt => ("Pull the left trigger (LT · L2 · ZL)", "اضغط الزناد الأيسر (LT · L2 · ZL)"),
            WizardStep::Rt => ("Pull the right trigger (RT · R2 · ZR)", "اضغط الزناد الأيمن (RT · R2 · ZR)"),
            WizardStep::Back => ("Press Back · Select · Share · Minus", "اضغط Back · Select · Share · −"),
            WizardStep::Start => ("Press Start · Options · Plus", "اضغط Start · Options · +"),
            WizardStep::L3 => ("Click the left stick (L3)", "اضغط على العصا اليسرى (L3)"),
            WizardStep::R3 => ("Click the right stick (R3)", "اضغط على العصا اليمنى (R3)"),
            WizardStep::Guide => ("Press the home button (Guide · PS · Home), or Skip", "اضغط زر الرئيسية (Guide · PS · Home) أو تخطَّ"),
            WizardStep::RightStickRight => ("Push the right stick to the RIGHT and hold", "ادفع العصا اليمنى إلى اليمين وثبّتها"),
            WizardStep::RightStickDown => ("Push the right stick DOWN and hold", "ادفع العصا اليمنى إلى الأسفل وثبّتها"),
        }
    }
}

/// Walks through [`WizardStep::ALL`], reading which button or axis the user
/// moves for each one. Pure: feed it readings with [`MapWizard::observe`].
#[cfg_attr(not(feature = "gui"), allow(dead_code))] // the wizard UI lives in the settings window
#[derive(Clone, Debug)]
pub struct MapWizard {
    pub vendor: u16,
    pub product: u16,
    pub name: String,
    ranges: Ranges,
    step: usize,
    /// Reading with nothing held, taken before each step.
    rest: Option<RawState>,
    /// A step just finished: wait until everything is released.
    waiting_release: bool,
    layout: Layout,
    lt_axis: Option<Axis>,
    rt_axis: Option<Axis>,
}

const STICK_AXES: [Axis; 4] = [Axis::Z, Axis::R, Axis::U, Axis::V];
const TRIGGER_AXES: [Axis; 4] = [Axis::Z, Axis::R, Axis::U, Axis::V];

#[cfg_attr(not(feature = "gui"), allow(dead_code))] // the wizard UI lives in the settings window
impl MapWizard {
    pub fn new(dev: &Device) -> MapWizard {
        MapWizard {
            vendor: dev.vendor,
            product: dev.product,
            name: dev.display_name(),
            ranges: dev.ranges,
            step: 0,
            rest: None,
            waiting_release: false,
            layout: dev.layout,
            lt_axis: None,
            rt_axis: None,
        }
    }

    /// The step to perform now, or `None` when finished.
    pub fn current(&self) -> Option<WizardStep> {
        WizardStep::ALL.get(self.step).copied()
    }

    /// `(steps done, total)`.
    pub fn progress(&self) -> (usize, usize) {
        (self.step.min(WizardStep::ALL.len()), WizardStep::ALL.len())
    }

    /// Waiting for the user to let go after the previous step?
    pub fn waiting_for_release(&self) -> bool {
        self.waiting_release
    }

    fn fraction(&self, raw: &RawState, a: Axis) -> f32 {
        let i = axis_index(a);
        let (min, max) = (self.ranges.min[i] as f32, self.ranges.max[i] as f32);
        if max <= min {
            return 0.5;
        }
        ((raw.axes[i] as f32 - min) / (max - min)).clamp(0.0, 1.0)
    }

    /// The axis (among `candidates`, minus `except`) that moved furthest from
    /// rest, if it moved more than `threshold` of its range.
    fn moved_axis(&self, raw: &RawState, rest: &RawState, candidates: &[Axis], except: &[Axis], threshold: f32) -> Option<Axis> {
        candidates
            .iter()
            .copied()
            .filter(|a| !except.contains(a))
            .map(|a| (a, (self.fraction(raw, a) - self.fraction(rest, a)).abs()))
            .filter(|(_, d)| *d > threshold)
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(a, _)| a)
    }

    fn at_rest(&self, raw: &RawState, rest: &RawState) -> bool {
        raw.buttons == rest.buttons
            && raw.pov == rest.pov
            && STICK_AXES
                .iter()
                .all(|a| (self.fraction(raw, *a) - self.fraction(rest, *a)).abs() < 0.2)
    }

    fn advance(&mut self) {
        self.step += 1;
        self.waiting_release = true;
    }

    /// Skip the current step, keeping the controller's default for it.
    pub fn skip(&mut self) {
        if self.current().is_some() {
            self.advance();
        }
    }

    /// Feed one reading of the controller.
    pub fn observe(&mut self, raw: &RawState) {
        let Some(rest) = self.rest else {
            self.rest = Some(*raw);
            return;
        };
        if self.waiting_release {
            if self.at_rest(raw, &rest) {
                self.waiting_release = false;
            }
            return;
        }
        let Some(step) = self.current() else { return };
        let newly = raw.buttons & !rest.buttons;
        let button = (newly != 0).then(|| newly.trailing_zeros() as u8 + 1);
        match step {
            WizardStep::RightStickRight => {
                let used: Vec<Axis> = [self.lt_axis, self.rt_axis].into_iter().flatten().collect();
                if let Some(a) = self.moved_axis(raw, &rest, &STICK_AXES, &used, 0.35) {
                    self.layout.right_stick.0 = a;
                    self.advance();
                }
            }
            WizardStep::RightStickDown => {
                let mut used: Vec<Axis> = [self.lt_axis, self.rt_axis].into_iter().flatten().collect();
                used.push(self.layout.right_stick.0);
                if let Some(a) = self.moved_axis(raw, &rest, &STICK_AXES, &used, 0.35) {
                    self.layout.right_stick.1 = a;
                    self.advance();
                }
            }
            WizardStep::Lt | WizardStep::Rt => {
                if let Some(n) = button {
                    if step == WizardStep::Lt {
                        self.layout.lt = n;
                    } else {
                        self.layout.rt = n;
                    }
                    self.advance();
                } else {
                    let used: Vec<Axis> = self.lt_axis.into_iter().collect();
                    if let Some(a) = self.moved_axis(raw, &rest, &TRIGGER_AXES, &used, 0.5) {
                        if step == WizardStep::Lt {
                            self.lt_axis = Some(a);
                        } else {
                            self.rt_axis = Some(a);
                        }
                        self.advance();
                    }
                }
            }
            _ => {
                if let Some(n) = button {
                    match step {
                        WizardStep::A => self.layout.face[0] = n,
                        WizardStep::B => self.layout.face[1] = n,
                        WizardStep::X => self.layout.face[2] = n,
                        WizardStep::Y => self.layout.face[3] = n,
                        WizardStep::Lb => self.layout.lb = n,
                        WizardStep::Rb => self.layout.rb = n,
                        WizardStep::Back => self.layout.back = n,
                        WizardStep::Start => self.layout.start = n,
                        WizardStep::L3 => self.layout.l3 = n,
                        WizardStep::R3 => self.layout.r3 = n,
                        WizardStep::Guide => self.layout.guide = Some(n),
                        _ => {}
                    }
                    self.advance();
                }
            }
        }
    }

    /// The layout learned so far (complete once `current()` is `None`).
    pub fn layout(&self) -> Layout {
        let mut layout = self.layout;
        layout.triggers = match (self.lt_axis, self.rt_axis) {
            (Some(l), Some(r)) => Some((l, r)),
            _ => None,
        };
        layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_device() -> Device {
        Device {
            id: 0,
            name: String::new(),
            vendor: 0x1234,
            product: 0x5678,
            family: Family::Generic,
            ranges: FULL,
            layout: Layout::for_family(Family::Generic),
        }
    }

    fn press(buttons: u32) -> RawState {
        RawState { buttons, ..centred() }
    }

    #[test]
    fn wizard_learns_every_button_and_the_right_stick() {
        let mut w = MapWizard::new(&test_device());
        w.observe(&centred()); // rest reading
        // An unusual pad: buttons numbered 13..1 for the 13 button steps.
        for (i, step) in WizardStep::ALL.iter().take(13).enumerate() {
            assert_eq!(w.current(), Some(*step));
            let n = 13 - i as u32;
            w.observe(&press(1 << (n - 1)));
            assert!(w.waiting_for_release(), "{step:?} waits for release");
            w.observe(&press(1 << (n - 1)));
            assert!(w.waiting_for_release(), "still held");
            w.observe(&centred());
        }
        let mut right = centred();
        right.axes[4] = 65535; // U moves right
        w.observe(&right);
        w.observe(&centred());
        let mut down = centred();
        down.axes[5] = 65535; // V moves down
        w.observe(&down);
        w.observe(&centred());
        assert_eq!(w.current(), None);
        let l = w.layout();
        assert_eq!(l.face, [13, 12, 11, 10]);
        assert_eq!((l.lb, l.rb, l.lt, l.rt), (9, 8, 7, 6));
        assert_eq!((l.back, l.start, l.l3, l.r3, l.guide), (5, 4, 3, 2, Some(1)));
        assert_eq!(l.right_stick, (Axis::U, Axis::V));
        assert_eq!(l.triggers, None, "digital triggers");
        assert!(l.is_valid());
    }

    #[test]
    fn wizard_detects_analog_triggers_and_allows_skipping() {
        let mut w = MapWizard::new(&test_device());
        w.observe(&centred());
        for _ in 0..6 {
            w.skip();
            w.observe(&centred());
        }
        assert_eq!(w.current(), Some(WizardStep::Lt));
        let mut lt = centred();
        lt.axes[4] = 65535; // U from 0 to full
        w.observe(&lt);
        w.observe(&centred());
        let mut rt = centred();
        rt.axes[5] = 65535; // V
        w.observe(&rt);
        w.observe(&centred());
        assert_eq!(w.layout().triggers, Some((Axis::U, Axis::V)));
        assert_eq!(w.layout().face, Layout::for_family(Family::Generic).face, "skipped steps keep the defaults");
    }

    #[test]
    fn wizard_ignores_buttons_held_from_the_start_and_small_jitter() {
        let mut w = MapWizard::new(&test_device());
        let mut stuck = centred();
        stuck.buttons = 1 << 4; // a button already held when the wizard opens
        w.observe(&stuck);
        let mut jitter = stuck;
        jitter.axes[2] = 34000;
        w.observe(&jitter);
        assert_eq!(w.current(), Some(WizardStep::A), "neither counts as a press");
        let mut both = stuck;
        both.buttons |= 1 << 2;
        w.observe(&both);
        assert_eq!(w.layout().face[0], 3);
    }

    #[test]
    fn learned_layouts_are_used_for_that_model_only() {
        let mut pads = GenericPads::default();
        let mut custom = Layout::for_family(Family::Generic);
        custom.face = [4, 3, 2, 1];
        pads.set_overrides(vec![(0x1234, 0x5678, custom)]);
        assert_eq!(pads.override_for(0x1234, 0x5678), Some(custom));
        assert_eq!(pads.override_for(0x1234, 0x9999), None);
        let mut broken = custom;
        broken.lb = 0;
        pads.set_overrides(vec![(0x1234, 0x5678, broken)]);
        assert_eq!(pads.override_for(0x1234, 0x5678), None, "an invalid layout is ignored");
    }

    const FULL: Ranges = Ranges { min: [0; 6], max: [65535; 6] };

    fn centred() -> RawState {
        RawState { axes: [32767, 32767, 32767, 32767, 0, 0], buttons: 0, pov: JOY_POVCENTERED }
    }

    #[test]
    fn families_are_recognised_from_usb_ids() {
        assert_eq!(Family::from_ids(0x054C, 0x05C4), Family::PlayStation4);
        assert_eq!(Family::from_ids(0x054C, 0x09CC), Family::PlayStation4);
        assert_eq!(Family::from_ids(0x054C, 0x0CE6), Family::PlayStation5);
        assert_eq!(Family::from_ids(0x054C, 0x0DF2), Family::PlayStation5);
        assert_eq!(Family::from_ids(0x057E, 0x2009), Family::SwitchPro);
        assert_eq!(Family::from_ids(0x2DC8, 0x6001), Family::Generic);
    }

    #[test]
    fn playstation_buttons_land_on_the_matching_xbox_positions() {
        let layout = Layout::for_family(Family::PlayStation5);
        let mut raw = centred();
        raw.buttons = 1 << 1; // Cross
        assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons, 0x1000, "Cross = A");
        raw.buttons = 1 << 2; // Circle
        assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons, 0x2000, "Circle = B");
        raw.buttons = 1; // Square
        assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons, 0x4000, "Square = X");
        raw.buttons = 1 << 3; // Triangle
        assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons, 0x8000, "Triangle = Y");
        raw.buttons = 1 << 12; // PS
        assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons, 0x0400, "PS = Guide");
        raw.buttons = (1 << 8) | (1 << 9); // Share + Options
        assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons, 0x0030, "Back + Start");
    }

    #[test]
    fn switch_pro_maps_by_position_not_by_letter() {
        let layout = Layout::for_family(Family::SwitchPro);
        let mut raw = centred();
        raw.buttons = 1; // Nintendo B, the bottom button
        assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons, 0x1000, "bottom = A");
        raw.buttons = 1 << 1; // Nintendo A, the right button
        assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons, 0x2000, "right = B");
        raw.buttons = 1 << 6; // ZL: digital trigger
        assert_eq!(to_gamepad(&raw, &FULL, &layout).bLeftTrigger, 255);
    }

    #[test]
    fn sticks_are_centred_scaled_and_y_points_up() {
        let layout = Layout::for_family(Family::Generic);
        let gp = to_gamepad(&centred(), &FULL, &layout);
        assert!(gp.sThumbLX.abs() <= 1 && gp.sThumbLY.abs() <= 1, "centre reads as zero");
        let mut raw = centred();
        raw.axes[0] = 65535; // full right
        raw.axes[1] = 0; // full up (winmm Y grows downwards)
        raw.axes[3] = 65535; // right stick full down
        let gp = to_gamepad(&raw, &FULL, &layout);
        assert_eq!(gp.sThumbLX, 32767);
        assert_eq!(gp.sThumbLY, 32767, "pushing up must be positive, like XInput");
        assert!(gp.sThumbRY <= -32767);
    }

    #[test]
    fn small_axis_ranges_still_reach_the_rails() {
        let ranges = Ranges { min: [0; 6], max: [255; 6] };
        let mut raw = RawState { axes: [255, 0, 128, 128, 0, 0], buttons: 0, pov: JOY_POVCENTERED };
        let gp = to_gamepad(&raw, &ranges, &Layout::for_family(Family::Generic));
        assert_eq!(gp.sThumbLX, 32767);
        raw.axes[0] = 0;
        assert_eq!(to_gamepad(&raw, &ranges, &Layout::for_family(Family::Generic)).sThumbLX, -32768);
        let broken = Ranges { min: [5; 6], max: [5; 6] };
        assert_eq!(to_gamepad(&raw, &broken, &Layout::for_family(Family::Generic)).sThumbLX, 0, "no divide by zero");
    }

    #[test]
    fn playstation_triggers_are_analog() {
        let layout = Layout::for_family(Family::PlayStation4);
        let mut raw = centred();
        raw.axes[4] = 65535; // L2 fully pulled
        raw.axes[5] = 32768; // R2 half way
        let gp = to_gamepad(&raw, &FULL, &layout);
        assert_eq!(gp.bLeftTrigger, 255);
        assert!((126..=129).contains(&gp.bRightTrigger));
    }

    #[test]
    fn hat_switch_becomes_the_dpad_including_diagonals() {
        let layout = Layout::for_family(Family::Generic);
        let mut raw = centred();
        for (pov, want) in [(0, 0x0001), (9000, 0x0008), (18000, 0x0002), (27000, 0x0004), (4500, 0x0009), (22500, 0x0006)] {
            raw.pov = pov;
            assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons & 0x000F, want, "pov {pov}");
        }
        raw.pov = JOY_POVCENTERED;
        assert_eq!(to_gamepad(&raw, &FULL, &layout).wButtons, 0);
    }

    #[test]
    fn names_hide_the_windows_driver_name() {
        assert_eq!(display_name(Family::Generic, "Microsoft PC-joystick driver", 0x3537, 0x1040), "Gamepad (3537:1040)");
        assert_eq!(display_name(Family::Generic, "8BitDo Pro 2", 0x2DC8, 0x6003), "8BitDo Pro 2");
        assert_eq!(display_name(Family::PlayStation5, "Wireless Controller", 0x054C, 0x0CE6), "PlayStation 5 (DualSense)");
    }

    #[test]
    fn listing_devices_never_fails_without_controllers() {
        // Runs on any machine: an empty list is fine, a crash is not.
        let _ = list_devices();
    }
}
