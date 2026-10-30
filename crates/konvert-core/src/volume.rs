//! Loudness of the audio: louder or quieter by a number of decibels, or
//! every track brought to the loudness streaming services play at (EBU R128,
//! two passes of FFmpeg's `loudnorm`: one measures, the next one applies).

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::plan::ConversionPlan;
use crate::probe::TrackKind;
use crate::request::Volume;

/// Integrated loudness of a normalized track, in LUFS.
pub const TARGET_LUFS: f64 = -16.0;
/// Highest true peak of a normalized track, in dBTP.
pub const TARGET_PEAK: f64 = -1.5;
/// Loudness range kept as it is up to this many LU (wider ones are narrowed).
const TARGET_RANGE: f64 = 11.0;
/// The largest gain, either way, in dB.
pub const MAX_GAIN_DB: f32 = 20.0;
/// Peaks of louder audio are held under this level (-1 dBFS).
const LIMIT: f64 = 0.891;

/// Refuses a gain outside ±[`MAX_GAIN_DB`].
pub fn check(volume: Option<Volume>) -> Result<(), crate::error::CoreError> {
    match volume {
        Some(Volume::Gain { db }) if !(db.is_finite() && db.abs() <= MAX_GAIN_DB) => {
            Err(crate::error::CoreError::UnsupportedConversion {
                details: format!("a gain of {db} dB is out of range"),
            })
        }
        _ => Ok(()),
    }
}

/// What the measuring pass found about one track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    pub integrated: f64,
    pub true_peak: f64,
    pub range: f64,
    pub threshold: f64,
    pub offset: f64,
}

/// One audio track of the output, as it is read to be measured: the file,
/// the stream (`-map`), where reading starts and for how long.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub path: PathBuf,
    pub map: String,
    pub start_secs: Option<f64>,
    pub length_secs: Option<f64>,
}

/// The audio tracks of the output, in order: the file's own, then the added one.
/// For a join: the sound of every file that has some, in order.
pub fn sources(plan: &ConversionPlan, input: &std::path::Path) -> Vec<Source> {
    let start = plan.trim.map(|(start, _)| start).filter(|s| *s > 0.0);
    let length = plan.trim.map(|(_, length)| length);
    let own: Vec<u32> = match (plan.audio.is_some(), plan.tracks.is_empty()) {
        (false, _) => Vec::new(),
        (true, true) => plan.audio_stream.into_iter().collect(),
        (true, false) => plan
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .map(|t| t.index)
            .collect(),
    };
    let mut sources: Vec<Source> = own
        .into_iter()
        .map(|index| Source {
            path: input.to_path_buf(),
            map: format!("0:{index}"),
            start_secs: start,
            length_secs: length,
        })
        .collect();
    if let Some(added) = &plan.added_audio {
        sources.push(Source {
            path: added.path.clone(),
            map: "0:a:0".into(),
            start_secs: Some(-added.delay_secs).filter(|s| *s > 0.0),
            length_secs: length,
        });
    }
    sources
}

/// The `-filter:a:N` of one track (`measured`: what the first pass found,
/// for a normalization in one linear step).
pub fn filter(volume: Volume, measured: Option<&Loudness>) -> String {
    match volume {
        // Louder: the peaks that would clip are held down instead.
        Volume::Gain { db } if db > 0.0 => {
            format!("volume={db}dB,alimiter=limit={LIMIT}:level=false")
        }
        Volume::Gain { db } => format!("volume={db}dB"),
        Volume::Normalize => {
            let mut f = format!("loudnorm=I={TARGET_LUFS}:TP={TARGET_PEAK}");
            match measured {
                Some(m) => {
                    // The range is kept when it is wider: films stay films.
                    let range = m.range.clamp(TARGET_RANGE, 50.0);
                    let _ = write!(
                        f,
                        ":LRA={range}:measured_I={}:measured_TP={}:measured_LRA={}\
                         :measured_thresh={}:offset={}:linear=true",
                        m.integrated, m.true_peak, m.range, m.threshold, m.offset
                    );
                }
                None => {
                    let _ = write!(f, ":LRA={TARGET_RANGE}");
                }
            }
            f
        }
    }
}

/// The filter of a measuring pass, which prints its findings as JSON.
pub fn measuring_filter() -> String {
    format!("loudnorm=I={TARGET_LUFS}:TP={TARGET_PEAK}:LRA={TARGET_RANGE}:print_format=json")
}

/// Reads the summary a measuring pass prints at its end. `None` when it is
/// missing or not finite (a silent track): the track is then normalized in
/// one pass.
pub fn parse(stderr: &str) -> Option<Loudness> {
    let start = stderr.rfind('{')?;
    let end = start + stderr[start..].find('}')?;
    let json: serde_json::Value = serde_json::from_str(&stderr[start..=end]).ok()?;
    let number = |key: &str| {
        json.get(key)?
            .as_str()?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    };
    Some(Loudness {
        integrated: number("input_i")?,
        true_peak: number("input_tp")?,
        range: number("input_lra")?,
        threshold: number("input_thresh")?,
        offset: number("target_offset")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMMARY: &str = r#"[Parsed_loudnorm_0 @ 000001]
{
	"input_i" : "-27.61",
	"input_tp" : "-4.47",
	"input_lra" : "18.06",
	"input_thresh" : "-39.20",
	"output_i" : "-16.58",
	"output_tp" : "-1.50",
	"output_lra" : "14.78",
	"output_thresh" : "-27.71",
	"normalization_type" : "dynamic",
	"target_offset" : "0.58"
}
[out#0/null @ 000002] video:0KiB audio:1KiB"#;

    #[test]
    fn measurements_are_read_from_the_summary() {
        let m = parse(SUMMARY).unwrap();
        assert_eq!(
            m,
            Loudness {
                integrated: -27.61,
                true_peak: -4.47,
                range: 18.06,
                threshold: -39.2,
                offset: 0.58,
            }
        );
        // A silent track measures -inf: normalized in one pass.
        assert!(parse(&SUMMARY.replace("\"-27.61\"", "\"-inf\"")).is_none());
        assert!(parse("Error opening input").is_none());
    }

    #[test]
    fn filters_follow_the_choice() {
        assert_eq!(filter(Volume::Gain { db: -6.0 }, None), "volume=-6dB");
        assert_eq!(
            filter(Volume::Gain { db: 3.5 }, None),
            "volume=3.5dB,alimiter=limit=0.891:level=false"
        );
        assert_eq!(
            filter(Volume::Normalize, None),
            "loudnorm=I=-16:TP=-1.5:LRA=11"
        );
        let m = parse(SUMMARY).unwrap();
        assert_eq!(
            filter(Volume::Normalize, Some(&m)),
            "loudnorm=I=-16:TP=-1.5:LRA=18.06:measured_I=-27.61:measured_TP=-4.47\
             :measured_LRA=18.06:measured_thresh=-39.2:offset=0.58:linear=true"
        );
    }
}
