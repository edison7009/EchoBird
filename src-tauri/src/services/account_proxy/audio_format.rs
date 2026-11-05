use super::ProxyError;
use axum::http::StatusCode;
pub(super) fn pcm(audio: &[u8]) -> Result<&[u8], ProxyError> {
    let invalid = || {
        ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Account speech requires 16 kHz mono PCM16 WAV audio",
        )
    };
    if audio.len() <= 44
        || &audio[..4] != b"RIFF"
        || &audio[8..16] != b"WAVEfmt "
        || &audio[36..40] != b"data"
    {
        return Err(invalid());
    }
    let word = |at| u16::from_le_bytes([audio[at], audio[at + 1]]);
    let dword =
        |at| u32::from_le_bytes([audio[at], audio[at + 1], audio[at + 2], audio[at + 3]]) as usize;
    if dword(4) != audio.len() - 8
        || dword(16) != 16
        || word(20) != 1
        || word(22) != 1
        || dword(24) != 16_000
        || dword(28) != 32_000
        || word(32) != 2
        || word(34) != 16
        || dword(40) != audio.len() - 44
        || !(audio.len() - 44).is_multiple_of(2)
    {
        return Err(invalid());
    }
    Ok(&audio[44..])
}
