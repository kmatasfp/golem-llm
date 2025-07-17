use golem_stt::golem::stt::languages::{Guest as LanguageGuest, LanguageInfo};

use golem_stt::golem::stt::transcription::{
    FailedTranscription as WitFailedTranscription, Guest as TranscriptionGuest,
    MultiTranscriptionResult as WitMultiTranscriptionResult,
    TranscriptionRequest as WitTranscriptionRequest, TranscriptionResult as WitTranscriptionResult,
};

mod aws;
mod aws_client;
mod client;
mod error;

#[allow(unused)]
struct Component;
