#[allow(non_camel_case_types)]
#[allow(unused)]
#[derive(Debug, Clone)]
pub enum AudioFormat {
    wav,
    mp3,
    flac,
    ogg,
}

impl core::fmt::Display for AudioFormat {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string_representation = match self {
            AudioFormat::wav => "wav",
            AudioFormat::mp3 => "mp3",
            AudioFormat::flac => "flac",
            AudioFormat::ogg => "ogg",
        };
        write!(fmt, "{string_representation}")
    }
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub format: AudioFormat,
    pub sample_rate_hertz: Option<u32>,
    pub channels: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct DiarizationConfig {
    pub enabled: bool,
    pub max_speakers: u8,
}

#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    pub language: Option<String>,
    pub model: Option<String>,
    pub diarization: Option<DiarizationConfig>,
    pub enable_multi_channel: bool,
    pub vocabulary: Vec<String>,
}

pub struct TranscriptionRequest {
    pub request_id: String,
    pub audio: Vec<u8>,
    pub audio_config: AudioConfig,
    pub transcription_config: Option<TranscriptionConfig>,
}
