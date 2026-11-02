//! Hardware video encoders: the graphics chip encodes H.264 or AV1 instead of
//! OpenH264/SVT-AV1 on the processor.
//!
//! The bundled FFmpeg builds contain the encoders of every vendor (NVIDIA
//! NVENC, Intel Quick Sync, AMD AMF, VA-API on Linux, VideoToolbox on macOS),
//! but only some of them work on a given computer: [`detect`] tries each one
//! that makes sense on this platform with a tiny encode (a few frames of a
//! generated picture, written nowhere) and keeps those that succeed.
//!
//! The planner uses a detected encoder when the request carries one
//! ([`crate::ConversionRequest::hardware`]); the conversion falls back to the
//! software encoder by itself when the hardware one fails
//! ([`crate::ConversionPlan::without_hardware`]).
//!
//! Everything stays local: the encoders talk to the graphics driver only.

use std::collections::HashSet;
use std::ffi::OsString;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::binaries::Binaries;
use crate::formats::VideoCodec;
use crate::process;

/// Who makes the hardware encoder (the FFmpeg encoder family).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Accelerator {
    /// NVIDIA graphics cards (`*_nvenc`).
    Nvenc,
    /// Intel graphics (`*_qsv`).
    Qsv,
    /// AMD graphics cards (`*_amf`).
    Amf,
    /// Intel and AMD graphics on Linux with the open drivers (`*_vaapi`).
    Vaapi,
    /// Every Mac (`*_videotoolbox`).
    VideoToolbox,
}

impl Accelerator {
    /// The encoder families tried on this platform, in order of preference.
    pub fn on_this_platform() -> &'static [Accelerator] {
        if cfg!(target_os = "macos") {
            &[Accelerator::VideoToolbox]
        } else if cfg!(target_os = "windows") {
            &[Accelerator::Nvenc, Accelerator::Qsv, Accelerator::Amf]
        } else {
            // VA-API last: Quick Sync and NVENC are faster where they work.
            &[
                Accelerator::Nvenc,
                Accelerator::Qsv,
                Accelerator::Amf,
                Accelerator::Vaapi,
            ]
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Accelerator::Nvenc => "nvenc",
            Accelerator::Qsv => "qsv",
            Accelerator::Amf => "amf",
            Accelerator::Vaapi => "vaapi",
            Accelerator::VideoToolbox => "videotoolbox",
        }
    }
}

/// One hardware encoder that works on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HardwareEncoder {
    pub accelerator: Accelerator,
    pub codec: VideoCodec,
    /// VA-API only: the graphics device (`/dev/dri/renderD128`).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub device: Option<String>,
}

impl HardwareEncoder {
    /// FFmpeg's name of the encoder (`h264_nvenc`), if the codec has hardware
    /// encoders at all.
    pub fn name(&self) -> Option<String> {
        let codec = match self.codec {
            VideoCodec::H264 => "h264",
            VideoCodec::Av1 => "av1",
            _ => return None,
        };
        Some(format!("{codec}_{}", self.accelerator.suffix()))
    }
}

/// Codecs that hardware may encode for Konvert's formats.
const CODECS: [VideoCodec; 2] = [VideoCodec::H264, VideoCodec::Av1];

/// The hardware encoders found on this computer, best first for each codec.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Hardware {
    pub encoders: Vec<HardwareEncoder>,
}

impl Hardware {
    /// The preferred encoder of `codec`, if one works here.
    pub fn encoder_for(&self, codec: VideoCodec) -> Option<&HardwareEncoder> {
        self.encoders.iter().find(|e| e.codec == codec)
    }
}

/// Encoders worth trying here: the platform's families × the codecs, when the
/// bundled FFmpeg has them (`ffmpeg -encoders`), in order of preference.
pub fn candidates(engine: &HashSet<String>, vaapi_device: Option<&str>) -> Vec<HardwareEncoder> {
    let mut found = Vec::new();
    for codec in CODECS {
        for &accelerator in Accelerator::on_this_platform() {
            let device = match accelerator {
                Accelerator::Vaapi => match vaapi_device {
                    Some(device) => Some(device.to_owned()),
                    None => continue,
                },
                _ => None,
            };
            let encoder = HardwareEncoder {
                accelerator,
                codec,
                device,
            };
            if encoder.name().is_some_and(|name| engine.contains(&name)) {
                found.push(encoder);
            }
        }
    }
    found
}

/// The first render node of the graphics drivers (Linux), for VA-API.
pub fn vaapi_device() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    (128..136)
        .map(|n| format!("/dev/dri/renderD{n}"))
        .find(|path| std::path::Path::new(path).exists())
}

/// Arguments of the trial encode of `encoder`: a few frames of a generated
/// picture, encoded and thrown away (`-f null`).
pub fn probe_args(encoder: &HardwareEncoder) -> Vec<OsString> {
    let name = encoder.name().unwrap_or_default();
    let mut args: Vec<String> = ["-hide_banner", "-nostdin", "-loglevel", "error"]
        .map(String::from)
        .to_vec();
    if let Some(device) = &encoder.device {
        args.extend(["-vaapi_device".to_owned(), device.clone()]);
    }
    // A generated picture: no file is read. 640×360, larger than every
    // encoder's minimum size.
    args.extend(
        [
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=30",
            "-frames:v",
            "10",
        ]
        .map(String::from),
    );
    if encoder.accelerator == Accelerator::Vaapi {
        args.extend(["-vf", "format=nv12,hwupload"].map(String::from));
    } else {
        args.extend(["-pix_fmt", "nv12"].map(String::from));
    }
    args.extend(["-c:v", &name, "-b:v", "1000k", "-f", "null", "-"].map(String::from));
    args.into_iter().map(OsString::from).collect()
}

/// How long a trial encode may take (a driver that hangs must not block Konvert).
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// The hardware encoders that work on this computer: each candidate of the
/// bundled FFmpeg (`engine` = `ffmpeg -encoders`) is tried once, one after
/// the other (graphics drivers limit how many sessions run at once).
pub async fn detect(bins: &Binaries, engine: &HashSet<String>) -> Hardware {
    let device = vaapi_device();
    let mut encoders = Vec::new();
    for candidate in candidates(engine, device.as_deref()) {
        if works(bins, &candidate).await {
            encoders.push(candidate);
        }
    }
    Hardware { encoders }
}

async fn works(bins: &Binaries, encoder: &HardwareEncoder) -> bool {
    let mut cmd = process::engine_command(&bins.ffmpeg);
    cmd.args(probe_args(encoder));
    let Ok(child) = process::spawn(&mut cmd) else {
        return false;
    };
    // Dropping the child on timeout kills it (`kill_on_drop`).
    match tokio::time::timeout(PROBE_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output.status.success(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn encoder_names_follow_ffmpeg() {
        let encoder = |accelerator, codec| HardwareEncoder {
            accelerator,
            codec,
            device: None,
        };
        let name = |accelerator, codec| encoder(accelerator, codec).name();
        assert_eq!(
            name(Accelerator::Nvenc, VideoCodec::H264).as_deref(),
            Some("h264_nvenc")
        );
        assert_eq!(
            name(Accelerator::Qsv, VideoCodec::Av1).as_deref(),
            Some("av1_qsv")
        );
        assert_eq!(
            name(Accelerator::Amf, VideoCodec::H264).as_deref(),
            Some("h264_amf")
        );
        assert_eq!(
            name(Accelerator::Vaapi, VideoCodec::Av1).as_deref(),
            Some("av1_vaapi")
        );
        assert_eq!(
            name(Accelerator::VideoToolbox, VideoCodec::H264).as_deref(),
            Some("h264_videotoolbox")
        );
        // No hardware encoders for the other codecs.
        assert_eq!(name(Accelerator::Nvenc, VideoCodec::Vp9), None);
        assert_eq!(name(Accelerator::Qsv, VideoCodec::Gif), None);
    }

    #[test]
    fn candidates_are_the_engines_encoders_of_this_platform_in_order() {
        let all = engine(&[
            "h264_nvenc",
            "h264_qsv",
            "h264_amf",
            "h264_vaapi",
            "h264_videotoolbox",
            "av1_nvenc",
            "av1_qsv",
            "av1_amf",
            "av1_vaapi",
            "libopenh264",
        ]);
        let found = candidates(&all, Some("/dev/dri/renderD128"));
        let names: Vec<String> = found.iter().filter_map(HardwareEncoder::name).collect();
        let expected: &[&str] = if cfg!(target_os = "macos") {
            &["h264_videotoolbox"]
        } else if cfg!(target_os = "windows") {
            &[
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
                "av1_nvenc",
                "av1_qsv",
                "av1_amf",
            ]
        } else {
            &[
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
                "h264_vaapi",
                "av1_nvenc",
                "av1_qsv",
                "av1_amf",
                "av1_vaapi",
            ]
        };
        assert_eq!(names, expected);
        // Without a render node, no VA-API.
        assert!(
            candidates(&all, None)
                .iter()
                .all(|e| e.accelerator != Accelerator::Vaapi)
        );
        // Without the encoders in the engine, nothing.
        assert!(candidates(&engine(&["libopenh264"]), None).is_empty());
    }

    #[test]
    fn the_preferred_encoder_of_a_codec_comes_first() {
        let hardware = Hardware {
            encoders: vec![
                HardwareEncoder {
                    accelerator: Accelerator::Nvenc,
                    codec: VideoCodec::H264,
                    device: None,
                },
                HardwareEncoder {
                    accelerator: Accelerator::Qsv,
                    codec: VideoCodec::H264,
                    device: None,
                },
                HardwareEncoder {
                    accelerator: Accelerator::Qsv,
                    codec: VideoCodec::Av1,
                    device: None,
                },
            ],
        };
        let accelerator = |codec| hardware.encoder_for(codec).map(|e| e.accelerator);
        assert_eq!(accelerator(VideoCodec::H264), Some(Accelerator::Nvenc));
        assert_eq!(accelerator(VideoCodec::Av1), Some(Accelerator::Qsv));
        assert_eq!(accelerator(VideoCodec::Vp9), None);
    }

    #[test]
    fn trial_encodes_read_no_file_and_write_nothing() {
        let args = |encoder: &HardwareEncoder| -> Vec<String> {
            probe_args(encoder)
                .into_iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let nvenc = args(&HardwareEncoder {
            accelerator: Accelerator::Nvenc,
            codec: VideoCodec::H264,
            device: None,
        });
        let joined = nvenc.join(" ");
        assert!(joined.contains("-f lavfi -i testsrc2="), "{joined}");
        assert!(joined.contains("-pix_fmt nv12 -c:v h264_nvenc"), "{joined}");
        assert!(joined.ends_with("-f null -"), "{joined}");
        assert!(!joined.contains("file:"), "{joined}");

        let vaapi = args(&HardwareEncoder {
            accelerator: Accelerator::Vaapi,
            codec: VideoCodec::Av1,
            device: Some("/dev/dri/renderD129".into()),
        })
        .join(" ");
        assert!(
            vaapi.contains("-vaapi_device /dev/dri/renderD129"),
            "{vaapi}"
        );
        assert!(
            vaapi.contains("-vf format=nv12,hwupload -c:v av1_vaapi"),
            "{vaapi}"
        );
    }
}
