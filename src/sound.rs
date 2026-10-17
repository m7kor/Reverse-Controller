//! In-memory audio synthesis and chimes for haptic/audio feedback.
//! 100% zero external dependencies, zero disk I/O, pure Win32 PlaySoundW.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundEffect {
    ListenStart,
    SendSuccess,
    KeyClick,
    RadialSnap,
}

#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(pszSound: *const u8, hmod: *mut std::ffi::c_void, fdwSound: u32) -> i32;
}

const SND_ASYNC: u32 = 0x0001;
const SND_NODEFAULT: u32 = 0x0002;
const SND_MEMORY: u32 = 0x0004;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static SFX_ENABLED: AtomicBool = AtomicBool::new(true);

/// Enable or disable sound effects like mechanical key clicks.
pub fn set_sfx_enabled(enabled: bool) {
    SFX_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Query whether sound effects are currently enabled.
pub fn is_sfx_enabled() -> bool {
    SFX_ENABLED.load(Ordering::Relaxed)
}

static WAV_LISTEN_START: OnceLock<Vec<u8>> = OnceLock::new();
static WAV_SEND_SUCCESS: OnceLock<Vec<u8>> = OnceLock::new();
static WAV_KEY_CLICK: OnceLock<Vec<u8>> = OnceLock::new();
static WAV_RADIAL_SNAP: OnceLock<Vec<u8>> = OnceLock::new();

/// Play a synthesized micro-sound asynchronously in the background.
/// Uses pre-cached static WAV buffers to ensure memory remains valid for PlaySoundW
/// and avoids spawning OS threads on every keystroke.
pub fn play_sfx(effect: SoundEffect) {
    if !is_sfx_enabled() {
        return;
    }

    let wav_data: &'static [u8] = match effect {
        SoundEffect::ListenStart => {
            WAV_LISTEN_START.get_or_init(|| generate_wav(523.25, 783.99, 0.08, 0.12))
        }
        SoundEffect::SendSuccess => {
            WAV_SEND_SUCCESS.get_or_init(|| generate_wav(880.0, 1046.5, 0.09, 0.14))
        }
        SoundEffect::KeyClick => {
            WAV_KEY_CLICK.get_or_init(|| generate_wav(1200.0, 900.0, 0.015, 0.04))
        }
        SoundEffect::RadialSnap => {
            WAV_RADIAL_SNAP.get_or_init(|| generate_wav(900.0, 1200.0, 0.02, 0.05))
        }
    };
    unsafe {
        PlaySoundW(
            wav_data.as_ptr(),
            std::ptr::null_mut(),
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

/// Generates a tiny, clean, in-memory 16-bit PCM RIFF WAV buffer with a smooth sine wave and envelope.
fn generate_wav(start_freq: f32, end_freq: f32, duration: f32, volume: f32) -> Vec<u8> {
    let sample_rate = 22050u32;
    let num_samples = (sample_rate as f32 * duration) as usize;
    let data_len = num_samples * 2;
    let file_len = 36 + data_len;

    let mut wav = Vec::with_capacity(44 + data_len);
    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(file_len as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    // fmt subchunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // subchunk size (16 for PCM)
    wav.extend_from_slice(&1u16.to_le_bytes());  // AudioFormat (1 = PCM)
    wav.extend_from_slice(&1u16.to_le_bytes());  // NumChannels (1 = Mono)
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // ByteRate
    wav.extend_from_slice(&2u16.to_le_bytes());  // BlockAlign
    wav.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample
    // data subchunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_len as u32).to_le_bytes());

    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let frac = i as f32 / num_samples as f32;
        let freq = start_freq + (end_freq - start_freq) * frac;
        let angle = t * freq * std::f32::consts::TAU;
        // Smooth sine wave with attack and exponential decay envelope (no clicks/pops)
        let env = (1.0 - frac).powf(1.8) * (1.0 - (-frac * 20.0).exp());
        let sample = (angle.sin() * volume * env * 32767.0) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }

    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_wav_header() {
        let wav = generate_wav(440.0, 880.0, 0.05, 0.1);
        assert!(wav.len() > 44);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
    }
}
