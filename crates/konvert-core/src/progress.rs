//! Parser for FFmpeg's machine-readable progress output (`-progress pipe:1`).
//!
//! FFmpeg periodically prints blocks of `key=value` lines terminated by a
//! `progress=continue` (or `progress=end`) line. The parser accumulates the keys
//! of the current block and yields a snapshot each time a block is complete.

use serde::Serialize;

/// Raw values of one completed progress block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgressSnapshot {
    /// Media time already written to the output, in seconds.
    pub out_time_secs: Option<f64>,
    /// Bytes written so far.
    pub total_size: Option<u64>,
    /// Processing speed as a multiple of real time (2.0 = twice as fast as playback).
    pub speed: Option<f64>,
    /// `true` for the final block (`progress=end`).
    pub finished: bool,
}

/// Incremental line parser.
#[derive(Debug, Default)]
pub struct ProgressParser {
    current: ProgressSnapshot,
}

impl ProgressParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one line; returns a snapshot when it closes a block.
    pub fn feed_line(&mut self, line: &str) -> Option<ProgressSnapshot> {
        let (key, value) = line.trim().split_once('=')?;
        let value = value.trim();
        match key {
            // `out_time_us` and (despite its name) `out_time_ms` are both microseconds.
            "out_time_us" | "out_time_ms" => {
                if let Ok(us) = value.parse::<i64>() {
                    self.current.out_time_secs = Some(us.max(0) as f64 / 1_000_000.0);
                }
            }
            "total_size" => self.current.total_size = value.parse().ok(),
            "speed" => {
                self.current.speed = value
                    .trim_end_matches('x')
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|s| s.is_finite() && *s > 0.0);
            }
            "progress" => {
                self.current.finished = value == "end";
                return Some(self.current.clone());
            }
            _ => {}
        }
        None
    }
}

/// Progress information ready to be shown to the user.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Completion between 0.0 and 1.0, when the input duration is known.
    pub fraction: Option<f64>,
    /// Estimated seconds left, when it can be estimated.
    pub eta_secs: Option<f64>,
    /// Processing speed as a multiple of real time.
    pub speed: Option<f64>,
    /// Media seconds processed so far.
    pub processed_secs: Option<f64>,
    /// Bytes written so far.
    pub output_bytes: Option<u64>,
}

impl Progress {
    /// Combines a snapshot with the total duration of the input.
    pub fn from_snapshot(snapshot: &ProgressSnapshot, duration_secs: Option<f64>) -> Self {
        let duration = duration_secs.filter(|d| d.is_finite() && *d > 0.0);
        let fraction = if snapshot.finished {
            Some(1.0)
        } else {
            match (snapshot.out_time_secs, duration) {
                (Some(done), Some(total)) => Some((done / total).clamp(0.0, 1.0)),
                _ => None,
            }
        };
        let eta_secs = match (snapshot.out_time_secs, duration, snapshot.speed) {
            _ if snapshot.finished => Some(0.0),
            (Some(done), Some(total), Some(speed)) => Some(((total - done) / speed).max(0.0)),
            _ => None,
        };
        Self {
            fraction,
            eta_secs,
            speed: snapshot.speed,
            processed_secs: snapshot.out_time_secs,
            output_bytes: snapshot.total_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "frame=0\nfps=0.00\nbitrate= 192.0kbits/s\ntotal_size=48000\n\
        out_time_us=2000000\nout_time_ms=2000000\nout_time=00:00:02.000000\n\
        dup_frames=0\ndrop_frames=0\nspeed=  40.5x\nprogress=continue\n";

    fn feed(parser: &mut ProgressParser, text: &str) -> Vec<ProgressSnapshot> {
        text.lines().filter_map(|l| parser.feed_line(l)).collect()
    }

    #[test]
    fn parses_a_block() {
        let mut parser = ProgressParser::new();
        let snaps = feed(&mut parser, BLOCK);
        assert_eq!(
            snaps,
            vec![ProgressSnapshot {
                out_time_secs: Some(2.0),
                total_size: Some(48_000),
                speed: Some(40.5),
                finished: false,
            }]
        );
    }

    #[test]
    fn handles_not_available_values() {
        let mut parser = ProgressParser::new();
        let snaps = feed(
            &mut parser,
            "out_time_us=N/A\ntotal_size=N/A\nspeed=N/A\nprogress=continue\n",
        );
        assert_eq!(snaps, vec![ProgressSnapshot::default()]);
    }

    #[test]
    fn negative_times_are_clamped() {
        let mut parser = ProgressParser::new();
        let snaps = feed(&mut parser, "out_time_us=-23220\nprogress=continue\n");
        assert_eq!(snaps[0].out_time_secs, Some(0.0));
    }

    #[test]
    fn computes_fraction_and_eta() {
        let snap = ProgressSnapshot {
            out_time_secs: Some(30.0),
            total_size: Some(1000),
            speed: Some(10.0),
            finished: false,
        };
        let p = Progress::from_snapshot(&snap, Some(120.0));
        assert_eq!(p.fraction, Some(0.25));
        assert_eq!(p.eta_secs, Some(9.0));
    }

    #[test]
    fn end_block_means_complete() {
        let mut parser = ProgressParser::new();
        let snaps = feed(&mut parser, "out_time_us=1000\nprogress=end\n");
        let p = Progress::from_snapshot(&snaps[0], None);
        assert_eq!(p.fraction, Some(1.0));
        assert_eq!(p.eta_secs, Some(0.0));
    }

    #[test]
    fn unknown_duration_gives_no_fraction() {
        let snap = ProgressSnapshot {
            out_time_secs: Some(5.0),
            ..Default::default()
        };
        assert_eq!(Progress::from_snapshot(&snap, None).fraction, None);
        assert_eq!(Progress::from_snapshot(&snap, Some(0.0)).fraction, None);
    }
}
