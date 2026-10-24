//! Screencast recording: Chromium sends JPEG frames as the page changes; they
//! are turned into a constant-rate WebM with ffmpeg, the way Playwright does.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use swiftbrowse_core::{CoreError, Result};

pub const FPS: f64 = 25.0;
/// About 4 minutes at 25 fps of distinct frames; older frames are dropped beyond it.
pub const MAX_FRAMES: usize = 6_000;

pub struct Frame {
    /// Seconds, from the browser's clock.
    pub timestamp: f64,
    pub jpeg: Vec<u8>,
}

/// What a recording produced.
pub enum Recording {
    Video(PathBuf),
    /// No ffmpeg: the frames as numbered JPEGs, with their timestamps.
    Frames {
        dir: PathBuf,
        count: usize,
    },
}

/// Writes the frames as `<stem>.webm` (or, without ffmpeg, `<stem>-frames/`).
/// `end` is the timestamp to hold the last frame until.
pub fn save(frames: &[Frame], end: f64, dir: &Path, stem: &str) -> Result<Recording> {
    if frames.is_empty() {
        return Err(CoreError::backend("no video frames were recorded (the page never painted)"));
    }
    let io = |e: std::io::Error| CoreError::backend(format!("cannot write video: {e}"));
    std::fs::create_dir_all(dir).map_err(io)?;
    let Some(ffmpeg) = crate::launch::discover_ffmpeg() else {
        let out = dir.join(format!("{stem}-frames"));
        std::fs::create_dir_all(&out).map_err(io)?;
        let mut index = String::new();
        for (i, f) in frames.iter().enumerate() {
            std::fs::write(out.join(format!("{i:05}.jpg")), &f.jpeg).map_err(io)?;
            index.push_str(&format!("{i:05}.jpg {:.3}\n", f.timestamp - frames[0].timestamp));
        }
        std::fs::write(out.join("frames.txt"), index).map_err(io)?;
        return Ok(Recording::Frames { dir: out, count: frames.len() });
    };
    let path = dir.join(format!("{stem}.webm"));
    let (w, h) = jpeg_size(&frames[0].jpeg).unwrap_or((800, 500));
    // Even dimensions for the encoder; gray padding like Playwright's.
    let (w, h) = (w + w % 2, h + h % 2);
    let mut child = Command::new(&ffmpeg)
        .args(["-loglevel", "error", "-f", "image2pipe", "-avioflags", "direct", "-fpsprobesize", "0"])
        .args(["-probesize", "32", "-analyzeduration", "0", "-c:v", "mjpeg", "-i", "pipe:0", "-y", "-an"])
        .args(["-r", &FPS.to_string(), "-c:v", "vp8", "-qmin", "0", "-qmax", "50", "-crf", "8"])
        .args(["-deadline", "realtime", "-speed", "8", "-b:v", "1M", "-threads", "1"])
        .args(["-vf", &format!("pad={w}:{h}:0:0:gray,crop={w}:{h}:0:0")])
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::backend(format!("cannot start ffmpeg ({}): {e}", ffmpeg.display())))?;
    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        // Constant frame rate: each frame repeats until the next one arrived.
        let start = frames[0].timestamp;
        let mut written = 0u64;
        for (i, f) in frames.iter().enumerate() {
            let until = frames.get(i + 1).map_or(end, |n| n.timestamp).max(f.timestamp);
            let target = ((until - start) * FPS).round() as u64;
            let repeat = target.saturating_sub(written).max(if i == 0 { 1 } else { 0 });
            for _ in 0..repeat {
                if stdin.write_all(&f.jpeg).is_err() {
                    break;
                }
            }
            written += repeat;
        }
    }
    let out = child.wait_with_output().map_err(|e| CoreError::backend(format!("ffmpeg failed: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::backend(format!(
            "ffmpeg could not encode the video: {}",
            String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("unknown error")
        )));
    }
    Ok(Recording::Video(path))
}

/// Width and height from a JPEG's start-of-frame marker.
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if matches!(marker, 0xC0..=0xC3) {
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            return Some((w, h));
        }
        i += 2 + len;
    }
    None
}
