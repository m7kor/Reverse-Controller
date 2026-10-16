//! Pure, time-based rest calibration; never changes settings itself.
use std::time::Duration;

pub struct RestCalibration {
    slot: u32,
    peaks: [f32; 2],
    samples: usize,
}

impl RestCalibration {
    pub fn new(slot: u32) -> Self {
        Self { slot, peaks: [0.0; 2], samples: 0 }
    }

    pub fn observe(&mut self, elapsed: Duration, slot: u32, axes: Option<[i16; 4]>) -> Result<Option<[u16; 2]>, &'static str> {
        if slot != self.slot { return Err("Controller changed. Start calibration again."); }
        let Some([lx, ly, rx, ry]) = axes else { return Err("Controller disconnected. Start calibration again."); };
        // Give the user time to release the stick used to reach the button.
        if elapsed < Duration::from_secs(1) { return Ok(None); }
        let magnitudes = [(lx as f32).hypot(ly as f32), (rx as f32).hypot(ry as f32)];
        if magnitudes.iter().any(|&v| v > 12000.0) {
            return Err("Stick movement is too large. Release both sticks and try again.");
        }
        for (peak, magnitude) in self.peaks.iter_mut().zip(magnitudes) {
            *peak = peak.max(magnitude);
        }
        self.samples += 1;
        if elapsed < Duration::from_secs(4) { return Ok(None); }
        if self.samples < 30 { return Err("Not enough samples. Keep Settings open and try again."); }
        Ok(Some(self.peaks.map(|peak| (peak.ceil() as u16 + 800).max(1500))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measures_radial_drift_with_margin_after_settling() {
        let mut c = RestCalibration::new(1);
        assert_eq!(c.observe(Duration::ZERO, 1, Some([30000; 4])), Ok(None));
        for tick in 10..40 {
            assert_eq!(c.observe(Duration::from_millis(tick * 100), 1, Some([3000, 4000, 0, 0])), Ok(None));
        }
        assert_eq!(c.observe(Duration::from_secs(4), 1, Some([0; 4])), Ok(Some([5800, 1500])));
    }
    #[test]
    fn rejects_disconnection_slot_change_motion_and_sparse_samples() {
        let mut c = RestCalibration::new(0);
        assert!(c.observe(Duration::ZERO, 0, None).is_err());
        assert!(c.observe(Duration::ZERO, 1, Some([0; 4])).is_err());
        assert!(c.observe(Duration::from_secs(1), 0, Some([i16::MIN; 4])).is_err());
        assert!(c.observe(Duration::from_secs(4), 0, Some([0; 4])).is_err());
    }
}
