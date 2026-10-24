use lukosbot_core::PlatformError;
use lukosbot_domain::{MediaRef, MediaSource, PlatformMessageId, SentMessage};

use super::{PLATFORM, mapping_error};

pub(super) fn materialized_bytes(
    media: &MediaRef,
    part: &'static str,
) -> Result<Vec<u8>, PlatformError> {
    match &media.source {
        MediaSource::Bytes(bytes) => Ok(bytes.to_vec()),
        _ => Err(PlatformError::Unsupported {
            platform: PLATFORM,
            part,
        }),
    }
}

pub(super) fn join_caption(text: &str, caption: Option<&str>) -> Option<String> {
    match (
        text.is_empty(),
        caption.is_some_and(|caption| !caption.is_empty()),
    ) {
        (true, false) => None,
        (false, false) => Some(text.to_owned()),
        (true, true) => caption.map(str::to_owned),
        (false, true) => Some(format!("{text}\n{}", caption.unwrap_or_default())),
    }
}

pub(super) fn sent_message(
    message: teloxide::types::Message,
) -> Result<SentMessage, PlatformError> {
    let message_id = PlatformMessageId::new(message.id.0.to_string())
        .map_err(|error| mapping_error(format!("sent message id: {error}")))?;
    Ok(SentMessage {
        platform: PLATFORM,
        message_id,
    })
}
