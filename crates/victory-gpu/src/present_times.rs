//! Optional host Present intervals (`VICTORY_FRAME_TIMES=<new CSV path>`).
//! These measure accepted DXGI presents, not display scanout or GPU duration.
//! File I/O runs on a bounded background queue; lost samples are explicit.

use std::io::{BufWriter, Write};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct Sample {
    sequence: u64,
    frame: u64,
    elapsed: Duration,
    interval: Duration,
    dropped: u64,
}

struct Clock {
    start: Instant,
    previous: Instant,
    sequence: u64,
    dropped: u64,
}

impl Clock {
    fn new(now: Instant) -> Self {
        Self {
            start: now,
            previous: now,
            sequence: 0,
            dropped: 0,
        }
    }

    fn sample(&mut self, frame: u64, now: Instant) -> Sample {
        self.sequence += 1;
        let sample = Sample {
            sequence: self.sequence,
            frame,
            elapsed: now.duration_since(self.start),
            interval: now.duration_since(self.previous),
            dropped: self.dropped,
        };
        self.previous = now;
        sample
    }
}

struct Recorder {
    sender: mpsc::SyncSender<Sample>,
    clock: Mutex<Option<Clock>>,
}

fn write_samples(writer: impl Write, receiver: mpsc::Receiver<Sample>) -> std::io::Result<()> {
    let mut writer = BufWriter::new(writer);
    writeln!(writer, "sample,frame,elapsed_ms,interval_ms,dropped")?;
    writer.flush()?;
    let mut flushed = Instant::now();
    loop {
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(s) => {
                writeln!(
                    writer,
                    "{},{},{:.6},{:.6},{}",
                    s.sequence,
                    s.frame,
                    s.elapsed.as_secs_f64() * 1000.0,
                    s.interval.as_secs_f64() * 1000.0,
                    s.dropped
                )?;
                if flushed.elapsed() >= Duration::from_secs(1) {
                    writer.flush()?;
                    flushed = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => writer.flush()?,
            Err(mpsc::RecvTimeoutError::Disconnected) => return writer.flush(),
        }
    }
}

pub(crate) fn present(frame: u64) {
    static RECORDER: OnceLock<Option<Recorder>> = OnceLock::new();
    let Some(recorder) = RECORDER.get_or_init(|| {
        let path = std::env::var_os("VICTORY_FRAME_TIMES").filter(|p| !p.is_empty())?;
        let file = match std::fs::File::create_new(&path) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(?path, %error, "cannot create host Present timing CSV");
                return None;
            }
        };
        let (sender, receiver) = mpsc::sync_channel(4096);
        if let Err(error) = std::thread::Builder::new()
            .name("victory-frame-times".into())
            .spawn(move || {
                if let Err(error) = write_samples(file, receiver) {
                    tracing::warn!(%error, "host Present timing writer stopped");
                }
            })
        {
            tracing::warn!(%error, "cannot start host Present timing writer");
            return None;
        }
        Some(Recorder {
            sender,
            clock: Mutex::new(None),
        })
    }) else {
        return;
    };
    let mut clock = recorder.clock.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let clock = clock.get_or_insert_with(|| Clock::new(now));
    if recorder.sender.try_send(clock.sample(frame, now)).is_err() {
        clock.dropped += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_follow_host_presents_not_guest_frame_numbers() {
        let now = Instant::now();
        let mut clock = Clock::new(now);
        let first = clock.sample(100, now);
        let next = clock.sample(104, now + Duration::from_millis(50));
        assert_eq!(first.interval, Duration::ZERO);
        assert_eq!(next.sequence, 2);
        assert_eq!(next.interval, Duration::from_millis(50));
        let (sender, receiver) = mpsc::sync_channel(2);
        sender.send(first).unwrap();
        sender.send(next).unwrap();
        drop(sender);
        let mut csv = Vec::new();
        write_samples(&mut csv, receiver).unwrap();
        assert_eq!(
            String::from_utf8(csv).unwrap(),
            "sample,frame,elapsed_ms,interval_ms,dropped\n1,100,0.000000,0.000000,0\n2,104,50.000000,50.000000,0\n"
        );
    }
}
