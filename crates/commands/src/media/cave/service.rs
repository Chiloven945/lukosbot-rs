use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use chrono::Utc;
use tokio::sync::Mutex;

use lukosbot_core::ports::{
    MediaLoadError, MediaLoadPolicy, MediaLoader, StateKey, StateScopeKind, StateStore,
    StateStoreError,
};
use lukosbot_domain::{
    ChatAddress, InboundMessage, InboundPart, MediaRef, MediaSource, OutboundMessage, OutboundPart,
};

use super::model::{CaveEntry, CaveImageBlob, CaveMeta};

const NS_META: &str = "cmd.cave.meta";
const NS_ENTRY: &str = "cmd.cave.entry";
const KEY_META: &str = "meta";

#[derive(Debug, thiserror::Error)]
pub enum CaveError {
    #[error("未检测到可保存的文本或图片")]
    Empty,

    #[error(transparent)]
    State(#[from] StateStoreError),

    #[error(transparent)]
    Media(#[from] MediaLoadError),

    #[error("回声洞图片数据损坏")]
    InvalidImage,

    #[error("回声洞数据无法编码")]
    Json(#[from] serde_json::Error),
}

pub struct CaveService {
    store: Arc<dyn StateStore>,
    lock: Mutex<()>,
    max_image_bytes: u64,
}

impl CaveService {
    pub fn new(store: Arc<dyn StateStore>, max_image_bytes: u64) -> Self {
        Self {
            store,
            lock: Mutex::new(()),
            max_image_bytes,
        }
    }

    pub fn max_image_bytes(&self) -> u64 {
        self.max_image_bytes
    }

    pub async fn get(&self, no: u32) -> Result<Option<CaveEntry>, CaveError> {
        let value = self
            .store
            .get(&StateKey::global(NS_ENTRY, no.to_string())?)
            .await?;
        Ok(value.and_then(|value| serde_json::from_value(value.value).ok()))
    }

    pub async fn random(&self) -> Result<Option<CaveEntry>, CaveError> {
        let entries = self.scan_entries().await?;
        if entries.is_empty() {
            return Ok(None);
        }
        Ok(Some(entries[fastrand::usize(..entries.len())].clone()))
    }

    pub async fn add(
        &self,
        message: &InboundMessage,
        current_text: Option<&str>,
        media: &dyn MediaLoader,
        policy: MediaLoadPolicy,
    ) -> Result<CaveEntry, CaveError> {
        let payload = self
            .extract_payload(message, current_text, media, policy)
            .await?;

        let _guard = self.lock.lock().await;
        let entries = self.scan_entries().await?;
        let no = entries
            .iter()
            .map(|entry| entry.no)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let used_quoted = message.quoted.as_ref().is_some_and(|quoted| {
            quoted.parts.iter().any(|part| {
                matches!(part, InboundPart::Text(text) if !text.trim().is_empty())
                    || matches!(part, InboundPart::Image(_))
            })
        });
        let source_message_id = if used_quoted {
            message
                .quoted
                .as_ref()
                .and_then(|quoted| quoted.message_id.as_ref())
        } else {
            message.message_id.as_ref()
        }
        .map(|id| id.as_str().to_owned());

        let entry = CaveEntry {
            no,
            uuid: random_uuid(),
            text: payload.text,
            image: payload.image,
            created_at: Utc::now().timestamp_millis(),
            created_by_platform: message.address.platform.as_str().to_owned(),
            created_by_user_id: Some(message.sender.id.as_str().to_owned()),
            created_by_chat: Some(format!(
                "{}:{}",
                message.address.platform.as_str(),
                message.address.chat_id.as_str()
            )),
            source_message_id,
        };

        self.store
            .put(
                StateKey::global(NS_ENTRY, no.to_string())?,
                serde_json::to_value(&entry)?,
                None,
            )
            .await?;

        if let Err(error) = self
            .write_meta(&entries, Some(no), no.saturating_add(1))
            .await
        {
            tracing::warn!(%error, no, "cave metadata update failed; it will be rebuilt");
        }
        Ok(entry)
    }

    pub async fn delete(&self, no: u32) -> Result<bool, CaveError> {
        let _guard = self.lock.lock().await;
        if self.get(no).await?.is_none() {
            return Ok(false);
        }
        self.store
            .delete(&StateKey::global(NS_ENTRY, no.to_string())?)
            .await?;
        let entries = self.scan_entries().await?;
        if let Err(error) = self.write_meta(&entries, None, no.saturating_add(1)).await {
            tracing::warn!(%error, no, "cave metadata update failed after delete");
        }
        Ok(true)
    }

    pub fn to_outbound(
        &self,
        _address: &ChatAddress,
        entry: &CaveEntry,
    ) -> Result<OutboundMessage, CaveError> {
        let meta = format!(
            "#{} - {}",
            entry.no,
            chrono::DateTime::<Utc>::from_timestamp_millis(entry.created_at)
                .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "未知".to_owned())
        );

        if let Some(image) = &entry.image {
            let bytes = STANDARD
                .decode(&image.base64)
                .map_err(|_| CaveError::InvalidImage)?;
            let caption = [Some(meta), entry.text.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(OutboundMessage {
                parts: vec![OutboundPart::Image {
                    media: MediaRef {
                        source: MediaSource::Bytes(Bytes::from(bytes)),
                        file_name: image.name.clone(),
                        mime_type: image.mime.clone(),
                        size_bytes: None,
                    },
                    caption: (!caption.is_empty()).then_some(caption),
                }],
            });
        }

        let mut parts = vec![OutboundPart::Text(meta)];
        if let Some(text) = entry.text.as_ref().filter(|text| !text.trim().is_empty()) {
            parts.push(OutboundPart::Text(text.clone()));
        }
        if parts.len() == 1 {
            parts.push(OutboundPart::Text("这个条目没有内容。".to_owned()));
        }
        Ok(OutboundMessage { parts })
    }

    async fn extract_payload(
        &self,
        message: &InboundMessage,
        current_text: Option<&str>,
        media: &dyn MediaLoader,
        policy: MediaLoadPolicy,
    ) -> Result<Payload, CaveError> {
        if let Some(quoted) = &message.quoted
            && let Some(payload) = self
                .payload_from_parts(&quoted.parts, media, policy)
                .await?
        {
            return Ok(payload);
        }

        let text = current_text
            .and_then(|text| (!text.trim().is_empty()).then_some(text.trim().to_owned()))
            .or_else(|| {
                current_text
                    .is_none()
                    .then(|| {
                        message
                            .text_parts()
                            .map(str::trim)
                            .filter(|text| !text.is_empty())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .filter(|text| !text.is_empty())
            });

        let image = self.first_image(&message.parts, media, policy).await?;
        if text.is_none() && image.is_none() {
            return Err(CaveError::Empty);
        }
        Ok(Payload { text, image })
    }

    async fn payload_from_parts(
        &self,
        parts: &[InboundPart],
        media: &dyn MediaLoader,
        policy: MediaLoadPolicy,
    ) -> Result<Option<Payload>, CaveError> {
        let text = parts
            .iter()
            .filter_map(|part| match part {
                InboundPart::Text(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let image = self.first_image(parts, media, policy).await?;
        if text.is_empty() && image.is_none() {
            Ok(None)
        } else {
            Ok(Some(Payload {
                text: (!text.is_empty()).then_some(text),
                image,
            }))
        }
    }

    async fn first_image(
        &self,
        parts: &[InboundPart],
        media: &dyn MediaLoader,
        policy: MediaLoadPolicy,
    ) -> Result<Option<CaveImageBlob>, CaveError> {
        for part in parts {
            let InboundPart::Image(reference) = part else {
                continue;
            };
            let loaded = media.load(reference, policy).await?;
            if loaded.bytes.len() as u64 > self.max_image_bytes {
                return Err(CaveError::Media(MediaLoadError::TooLarge {
                    limit: self.max_image_bytes,
                }));
            }
            return Ok(Some(CaveImageBlob {
                name: loaded.file_name.or_else(|| reference.file_name.clone()),
                mime: loaded.mime_type.or_else(|| reference.mime_type.clone()),
                base64: STANDARD.encode(loaded.bytes),
            }));
        }
        Ok(None)
    }

    async fn scan_entries(&self) -> Result<Vec<CaveEntry>, CaveError> {
        let entries = self
            .store
            .scan_namespace(StateScopeKind::Global, NS_ENTRY)
            .await?;
        Ok(entries
            .into_iter()
            .filter_map(|entry| {
                let key = entry.key.key.parse::<u32>().ok()?;
                let mut parsed: CaveEntry = serde_json::from_value(entry.value.value).ok()?;
                parsed.no = key;
                Some(parsed)
            })
            .collect())
    }

    async fn write_meta(
        &self,
        entries: &[CaveEntry],
        extra: Option<u32>,
        minimum_next: u32,
    ) -> Result<(), CaveError> {
        let mut active_numbers = entries.iter().map(|entry| entry.no).collect::<Vec<_>>();
        active_numbers.sort_unstable();
        if let Some(extra) = extra
            && !active_numbers.contains(&extra)
        {
            active_numbers.push(extra);
            active_numbers.sort_unstable();
        }
        let next_no = active_numbers
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(minimum_next)
            .max(1);
        let meta = CaveMeta {
            next_no,
            active_numbers,
        };
        self.store
            .put(
                StateKey::global(NS_META, KEY_META)?,
                serde_json::to_value(meta)?,
                None,
            )
            .await?;
        Ok(())
    }
}

struct Payload {
    text: Option<String>,
    image: Option<CaveImageBlob>,
}

fn random_uuid() -> String {
    format!("{:016x}{:016x}", fastrand::u64(..), fastrand::u64(..))
}
