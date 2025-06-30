/// The Deepgram Speech-to-Text API client for transcribing audio into the input language
///
/// https://developers.deepgram.com/reference/speech-to-text-api/listen
pub struct PreRecordedAudioApi<HC: HttpClient> {
    openai_api_token: Rc<str>,
    openai_api_base_url: Rc<str>,
    http_client: Rc<HC>,
}
