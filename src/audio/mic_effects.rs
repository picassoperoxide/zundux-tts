//! Real-time effects for the microphone passthrough.
//!
//! `module-loopback` cannot apply effects, so when the echo should also be applied
//! to the microphone we route it through our own process instead:
//!
//! ```text
//! parec (mic source) → EchoState::process → pacat (virtual sink)
//! ```
//!
//! Echo delay/decay are read from shared atomics on every chunk, so moving the
//! sliders in the settings takes effect without reconnecting the mic.

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

const SAMPLE_RATE: u32 = 48_000;
/// Longest supported echo delay. The settings slider tops out at 500 ms.
const MAX_DELAY_MS: u32 = 1_000;
/// ~10 ms of mono s16le audio per read keeps the added latency small.
const CHUNK_BYTES: usize = (SAMPLE_RATE as usize / 100) * 2;

/// Echo parameters shared between the UI thread and the audio thread.
pub struct MicEchoParams {
    delay_ms: AtomicU32,
    /// Decay multiplied by 1000 (atomics have no f64).
    decay_permille: AtomicU32,
}

impl MicEchoParams {
    pub fn new(delay_ms: u32, decay: f64) -> Self {
        let params = Self {
            delay_ms: AtomicU32::new(0),
            decay_permille: AtomicU32::new(0),
        };
        params.set(delay_ms, decay);
        params
    }

    pub fn set(&self, delay_ms: u32, decay: f64) {
        self.delay_ms
            .store(delay_ms.clamp(1, MAX_DELAY_MS), Ordering::Relaxed);
        let permille = (decay.clamp(0.0, 0.95) * 1000.0).round() as u32;
        self.decay_permille.store(permille, Ordering::Relaxed);
    }

    fn delay_samples(&self) -> usize {
        (SAMPLE_RATE as usize * self.delay_ms.load(Ordering::Relaxed) as usize) / 1000
    }

    fn decay(&self) -> f64 {
        self.decay_permille.load(Ordering::Relaxed) as f64 / 1000.0
    }
}

/// Streaming feedback echo: `y[n] = x[n] + decay * y[n - delay]`.
/// Same response as [`super::effects::apply_echo`], but processed chunk by chunk.
pub struct EchoState {
    history: Vec<i16>,
    pos: usize,
}

impl EchoState {
    pub fn new(max_delay_samples: usize) -> Self {
        Self {
            history: vec![0; max_delay_samples.max(1)],
            pos: 0,
        }
    }

    pub fn process(&mut self, samples: &mut [i16], delay_samples: usize, decay: f64) {
        let len = self.history.len();
        let delay = delay_samples.clamp(1, len);
        for sample in samples.iter_mut() {
            let past = self.history[(self.pos + len - delay) % len];
            let mixed = *sample as i64 + (past as f64 * decay) as i64;
            let out = mixed.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
            *sample = out;
            self.history[self.pos] = out;
            self.pos = (self.pos + 1) % len;
        }
    }
}

/// Running `parec → echo → pacat` pipeline. Stops when dropped.
pub struct MicEffectPipeline {
    parec: Child,
    pacat: Child,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MicEffectPipeline {
    pub fn start(source: &str, sink: &str, params: Arc<MicEchoParams>) -> Result<Self> {
        let format_args = [
            "--raw".to_string(),
            "--format=s16le".to_string(),
            format!("--rate={}", SAMPLE_RATE),
            "--channels=1".to_string(),
        ];

        let mut parec = Command::new("parec")
            .arg(format!("--device={}", source))
            .args(&format_args)
            .arg("--latency-msec=10")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn parec for mic effects")?;

        let mut pacat = match Command::new("pacat")
            .arg("--playback")
            .arg(format!("--device={}", sink))
            .args(&format_args)
            .arg("--latency-msec=30")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                let _ = parec.kill();
                let _ = parec.wait();
                return Err(e).context("Failed to spawn pacat for mic effects");
            }
        };

        let mut input = parec.stdout.take().context("parec has no stdout")?;
        let mut output = pacat.stdin.take().context("pacat has no stdin")?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);

        let worker = std::thread::Builder::new()
            .name("mic-effects".to_string())
            .spawn(move || {
                let max_delay = (SAMPLE_RATE * MAX_DELAY_MS / 1000) as usize;
                let mut echo = EchoState::new(max_delay);
                let mut buf = vec![0u8; CHUNK_BYTES + 1];
                // Bytes carried over when a read ends in the middle of a sample.
                let mut carry = 0usize;
                let mut samples: Vec<i16> = Vec::with_capacity(CHUNK_BYTES / 2);
                let mut out_bytes: Vec<u8> = Vec::with_capacity(CHUNK_BYTES);

                while !stop_flag.load(Ordering::Relaxed) {
                    let n = match input.read(&mut buf[carry..]) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let total = carry + n;
                    let usable = total - total % 2;

                    samples.clear();
                    samples.extend(
                        buf[..usable]
                            .chunks_exact(2)
                            .map(|b| i16::from_le_bytes([b[0], b[1]])),
                    );
                    echo.process(&mut samples, params.delay_samples(), params.decay());

                    out_bytes.clear();
                    for s in &samples {
                        out_bytes.extend_from_slice(&s.to_le_bytes());
                    }
                    if output.write_all(&out_bytes).is_err() {
                        break;
                    }

                    carry = total - usable;
                    if carry > 0 {
                        buf[0] = buf[usable];
                    }
                }
            })
            .context("Failed to start mic effects thread")?;

        tracing::info!("Mic effect pipeline started: {} → echo → {}", source, sink);
        Ok(Self {
            parec,
            pacat,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for MicEffectPipeline {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Killing the processes unblocks the worker's read/write.
        let _ = self.parec.kill();
        let _ = self.pacat.kill();
        let _ = self.parec.wait();
        let _ = self.pacat.wait();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        tracing::info!("Mic effect pipeline stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_repeats_impulse_with_decay() {
        let mut echo = EchoState::new(16);
        let mut samples = vec![0i16; 12];
        samples[0] = 10_000;
        echo.process(&mut samples, 4, 0.5);
        assert_eq!(samples[0], 10_000);
        assert_eq!(samples[4], 5_000);
        assert_eq!(samples[8], 2_500);
        assert_eq!(samples[1], 0);
    }

    #[test]
    fn echo_state_carries_across_chunks() {
        let mut echo = EchoState::new(16);
        let mut first = vec![0i16; 3];
        first[0] = 8_000;
        echo.process(&mut first, 4, 0.5);
        let mut second = vec![0i16; 3];
        echo.process(&mut second, 4, 0.5);
        // Sample index 4 overall = index 1 of the second chunk.
        assert_eq!(second[1], 4_000);
    }

    #[test]
    fn echo_clamps_instead_of_wrapping() {
        let mut echo = EchoState::new(4);
        let mut samples = vec![i16::MAX, 0, i16::MAX];
        echo.process(&mut samples, 2, 0.9);
        assert_eq!(samples[2], i16::MAX);
    }

    #[test]
    fn params_clamp_to_supported_range() {
        let params = MicEchoParams::new(5_000, 2.0);
        assert_eq!(
            params.delay_samples(),
            (SAMPLE_RATE * MAX_DELAY_MS / 1000) as usize
        );
        assert!((params.decay() - 0.95).abs() < 1e-9);
    }
}
