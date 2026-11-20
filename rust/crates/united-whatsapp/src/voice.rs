//! Call audio bridged to PipeWire's CLI tools.
//!
//! WhatsApp calls carry 16 kHz mono PCM here (the MLOW codec path in
//! `whatsapp-rust`), and `pw-record`/`pw-cat` already capture and play exactly
//! that on every PipeWire desktop. Spawning them only for the length of a call
//! keeps idle memory at zero and avoids linking ALSA/CPAL.

use std::io::{Read as _, Write as _};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use whatsapp_rust::voip::audio::{AudioSink, AudioSource};

const RATE: u32 = 16_000;
/// 60 ms at 16 kHz, the frame size the call engine expects.
const FRAME_SAMPLES: usize = 960;
const FRAME_BYTES: usize = FRAME_SAMPLES * 2;

/// One capture stream; frames are 60 ms of little-endian i16.
pub struct PipeWireMic {
    rx: async_channel::Receiver<Vec<i16>>,
    child: Arc<Mutex<Child>>,
}

impl AudioSource for PipeWireMic {
    fn frames(&self) -> async_channel::Receiver<Vec<i16>> {
        self.rx.clone()
    }
}

impl Drop for PipeWireMic {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// One playback stream; `playout` accepts 60 ms frames and drops them when the
/// speaker falls behind (a call tolerates loss).
pub struct PipeWireSpeaker {
    tx: async_channel::Sender<Vec<i16>>,
    child: Arc<Mutex<Child>>,
}

impl AudioSink for PipeWireSpeaker {
    fn playout(&self) -> async_channel::Sender<Vec<i16>> {
        self.tx.clone()
    }
}

impl Drop for PipeWireSpeaker {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `pw-record` at the call's native format.
fn record_command() -> Command {
    let mut command = Command::new("pw-record");
    command.args([
        "--raw",
        "--format",
        "s16",
        "--rate",
        &RATE.to_string(),
        "--channels",
        "1",
        "-",
    ]);
    command
}

/// `pw-cat --playback` at the call's native format.
fn play_command() -> Command {
    let mut command = Command::new("pw-cat");
    command.args([
        "--playback",
        "-",
        "--raw",
        "--format",
        "s16",
        "--rate",
        &RATE.to_string(),
        "--channels",
        "1",
    ]);
    command
}

/// Open the default PipeWire source and speaker. Fails fast when the tools or
/// the sound server are missing, so the UI can report "no audio" instead of
/// ringing into a silent call.
pub fn open() -> Result<(PipeWireMic, PipeWireSpeaker)> {
    let mut recorder = record_command()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| anyhow!("pw-record: {err}"))?;
    let mut stdout = recorder
        .stdout
        .take()
        .ok_or_else(|| anyhow!("pw-record has no stdout"))?;
    let recorder = Arc::new(Mutex::new(recorder));
    let (mic_tx, mic_rx) = async_channel::bounded::<Vec<i16>>(8);
    std::thread::Builder::new()
        .name("united-mic".into())
        .spawn(move || {
            let mut bytes = [0u8; FRAME_BYTES];
            while stdout.read_exact(&mut bytes).is_ok() {
                let pcm = bytes
                    .chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                    .collect::<Vec<_>>();
                // Try-send: a stalled call must never block capture.
                let _ = mic_tx.try_send(pcm);
            }
        })?;

    let mut player = play_command()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| anyhow!("pw-cat: {err}"))?;
    let mut stdin = player
        .stdin
        .take()
        .ok_or_else(|| anyhow!("pw-cat has no stdin"))?;
    let player = Arc::new(Mutex::new(player));
    let (spk_tx, spk_rx) = async_channel::bounded::<Vec<i16>>(64);
    std::thread::Builder::new()
        .name("united-speaker".into())
        .spawn(move || {
            while let Ok(frame) = spk_rx.recv_blocking() {
                let mut bytes = Vec::with_capacity(frame.len() * 2);
                for sample in frame {
                    bytes.extend_from_slice(&sample.to_le_bytes());
                }
                if stdin.write_all(&bytes).is_err() {
                    break;
                }
            }
        })?;

    Ok((
        PipeWireMic {
            rx: mic_rx,
            child: recorder,
        },
        PipeWireSpeaker {
            tx: spk_tx,
            child: player,
        },
    ))
}
