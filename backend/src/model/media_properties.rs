// backend/src/model/media_properties.rs

use serde::{Deserialize, Serialize};
use std::fmt;
use serde_json::{Value, Map};

// Enum for Video Resolution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VideoResolution {
    #[default]
    Unknown,
    SD,
    P720,
    P1080,
    P2160, // 4K
    P4320, // 8K
}

impl fmt::Display for VideoResolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VideoResolution::SD => write!(f, "SD"),
            VideoResolution::P720 => write!(f, "720p"),
            VideoResolution::P1080 => write!(f, "1080p"),
            VideoResolution::P2160 => write!(f, "4K"),
            VideoResolution::P4320 => write!(f, "8K"),
            VideoResolution::Unknown => write!(f, "Unknown"),
        }
    }
}

// Enum for Video Codec
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VideoCodec {
    #[default]
    Other,
    H264,
    H265,
    MPEG4,
    VC1,
}

impl fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VideoCodec::H264 => write!(f, "x264"),
            VideoCodec::H265 => write!(f, "x265"),
            VideoCodec::MPEG4 => write!(f, "MPEG4"),
            VideoCodec::VC1 => write!(f, "VC-1"),
            VideoCodec::Other => write!(f, "Other"),
        }
    }
}

// Enum for Audio Codec
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AudioCodec {
    #[default]
    Other,
    AAC,
    AC3,
    EAC3,
    DTS,
    TrueHD,
    FLAC,
}

impl fmt::Display for AudioCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioCodec::AAC => write!(f, "AAC"),
            AudioCodec::AC3 => write!(f, "AC3"),
            AudioCodec::EAC3 => write!(f, "E-AC3"),
            AudioCodec::DTS => write!(f, "DTS"),
            AudioCodec::TrueHD => write!(f, "TrueHD"),
            AudioCodec::FLAC => write!(f, "FLAC"),
            AudioCodec::Other => write!(f, "Other"),
        }
    }
}

// Enum for Audio Channels
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AudioChannels {
    #[default]
    Unknown,
    Mono,
    Stereo,
    Surround51,
    Surround71,
}

impl fmt::Display for AudioChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioChannels::Mono => write!(f, "1.0"),
            AudioChannels::Stereo => write!(f, "2.0"),
            AudioChannels::Surround51 => write!(f, "5.1"),
            AudioChannels::Surround71 => write!(f, "7.1"),
            AudioChannels::Unknown => write!(f, "Unknown"),
        }
    }
}

// NEW: Enum for Video Dynamic Range
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VideoDynamicRange {
    #[default]
    SDR,
    HDR,
    HDR10,
    HLG,
    DV, // Dolby Vision
}

impl fmt::Display for VideoDynamicRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VideoDynamicRange::SDR => write!(f, "SDR"),
            VideoDynamicRange::HDR => write!(f, "HDR"),
            VideoDynamicRange::HDR10 => write!(f, "HDR10"),
            VideoDynamicRange::HLG => write!(f, "HLG"),
            VideoDynamicRange::DV => write!(f, "DV"),
        }
    }
}

/// A struct that holds all classified media quality features.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaQuality {
    pub resolution: VideoResolution,
    pub video_codec: VideoCodec,
    pub dynamic_range: VideoDynamicRange,
    pub audio_codec: AudioCodec,
    pub audio_channels: AudioChannels,
}

impl MediaQuality {
    /// Formats the quality features into a string suitable for filenames, e.g., "1080p | x265 | DTS | 5.1".
    /// Returns an empty string if no relevant features are available to display.
    pub fn format_for_filename(&self, separator: &str) -> String {
        let mut parts = Vec::new();

        if self.resolution != VideoResolution::Unknown {
            parts.push(self.resolution.to_string());
        }
        if self.video_codec != VideoCodec::Other {
            parts.push(self.video_codec.to_string());
        }
        // Only show dynamic range if it's not standard SDR.
        if self.dynamic_range != VideoDynamicRange::SDR {
            parts.push(self.dynamic_range.to_string());
        }
        if self.audio_codec != AudioCodec::Other {
            parts.push(self.audio_codec.to_string());
        }
        if self.audio_channels != AudioChannels::Unknown {
            parts.push(self.audio_channels.to_string());
        }

        parts.join(separator)
    }

    /// Extracts media quality information from an `ffprobe` info block.
    /// The `info_block` is expected to be a `serde_json::Value` object.
    pub fn from_ffprobe_info(info_block: &Value) -> Option<Self> {
        let video_info = info_block.get("video")?.as_object()?;
        // Assuming the first audio stream is the primary one.
        let audio_info = info_block.get("audio")?.as_object()?;

        // Helper to get a value by trying a prioritized list of field names.
        let get_value = |obj: &Map<String, Value>, fields: &[&str]| -> Option<Value> {
            for field in fields {
                if let Some(value) = obj.get(*field) {
                    if !value.is_null() {
                        return Some(value.clone());
                    }
                }
            }
            None
        };
        
        // 1. Classify video resolution from width
        let resolution = get_value(video_info, &["height", "coded_height"])
            .and_then(|v| v.as_u64())
            .map_or(VideoResolution::default(), |h| match h {
                _ if h >= 4300 => VideoResolution::P4320,
                _ if h >= 2100 => VideoResolution::P2160,
                _ if h >= 1000 => VideoResolution::P1080,
                _ if h >= 700 => VideoResolution::P720,
                _ => VideoResolution::SD,
            });

        // 2. Classify video codec
        let video_codec = get_value(video_info, &["codec_name"])
            .and_then(|v| v.as_str().map(str::to_lowercase))
            .map_or(VideoCodec::default(), |name| match name.as_str() {
                "h264" => VideoCodec::H264,
                "hevc" => VideoCodec::H265,
                "mpeg4" => VideoCodec::MPEG4,
                "vc1" => VideoCodec::VC1,
                _ => VideoCodec::default(),
            });

        // 3. Classify dynamic range
        let dynamic_range = {
            let tag_string = get_value(video_info, &["codec_tag_string"])
                .and_then(|v| v.as_str().map(str::to_lowercase));

            if tag_string == Some("dovi".to_string()) {
                VideoDynamicRange::DV
            } else {
                get_value(video_info, &["color_transfer"])
                    .and_then(|v| v.as_str().map(str::to_lowercase))
                    .map_or(VideoDynamicRange::SDR, |ct| match ct.as_str() {
                        "smpte2084" => VideoDynamicRange::HDR10,
                        "arib-std-b67" => VideoDynamicRange::HLG,
                        _ => VideoDynamicRange::SDR,
                    })
            }
        };
            
        // 4. Classify audio codec
        let audio_codec = get_value(audio_info, &["codec_name"])
            .and_then(|v| v.as_str().map(str::to_lowercase))
            .map_or(AudioCodec::default(), |name| match name.as_str() {
                "aac" => AudioCodec::AAC,
                "ac3" => AudioCodec::AC3,
                "eac3" => AudioCodec::EAC3,
                "dts" => AudioCodec::DTS,
                "truehd" => AudioCodec::TrueHD,
                "flac" => AudioCodec::FLAC,
                _ => AudioCodec::default(),
            });

        // 5. Classify audio channels
        let audio_channels = get_value(audio_info, &["channel_layout"])
            .and_then(|v| v.as_str().map(str::to_lowercase))
            .map_or(AudioChannels::default(), |layout| match layout.as_str() {
                l if l.starts_with("7.1") => AudioChannels::Surround71,
                l if l.starts_with("5.1") => AudioChannels::Surround51,
                "stereo" => AudioChannels::Stereo,
                "mono" => AudioChannels::Mono,
                _ => AudioChannels::default(),
            });

        Some(Self {
            resolution,
            video_codec,
            dynamic_range,
            audio_codec,
            audio_channels,
        })
    }
}