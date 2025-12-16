use anyhow::Result;
use async_trait::async_trait;
use serenity::all::{ChannelId, CreateAttachment, CreateEmbed, CreateMessage, Http, UserId};
use std::sync::Arc;

use crate::core::message_sender_hub::Sender;
use crate::model::{MessageOut, OutContentType};

use super::stack::DiscordStack;

pub struct DiscordSender {
    http: Arc<Http>,
}

impl DiscordSender {
    const MAX_CONTENT: usize = 2000;
    const MAX_EMBED_DESC: usize = 4096;

    pub fn new(stack: Arc<DiscordStack>) -> Self {
        Self {
            http: Arc::new(Http::new(&stack.token)),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    async fn send_to_channel(
        &self,
        ch: ChannelId,
        content: String,
        embeds: Vec<CreateEmbed>,
        files: Vec<CreateAttachment>,
    ) -> Result<()> {
        if content.chars().count() <= Self::MAX_CONTENT {
            let mut msg = CreateMessage::new();
            if !content.is_empty() {
                msg = msg.content(content);
            }
            if !embeds.is_empty() {
                msg = msg.embeds(embeds);
            }
            if !files.is_empty() {
                msg = msg.files(files);
            }
            ch.send_message(&*self.http, msg).await?;
            return Ok(());
        }

        // 超长：拆 4096 一段进 embed desc（对齐 Java） :contentReference[oaicite:37]{index=37}
        let chars: Vec<char> = content.chars().collect();
        let mut merged = embeds;

        let mut start = 0;
        while start < chars.len() {
            let end = usize::min(chars.len(), start + Self::MAX_EMBED_DESC);
            let part: String = chars[start..end].iter().collect();
            merged.push(CreateEmbed::new().description(part));
            start = end;
        }

        let mut msg = CreateMessage::new().content("");
        msg = msg.embeds(merged);
        if !files.is_empty() {
            msg = msg.files(files);
        }
        ch.send_message(&*self.http, msg).await?;
        Ok(())
    }
}

#[async_trait]
impl Sender for DiscordSender {
    async fn send(&self, out: MessageOut) -> Result<()> {
        // 对齐 Java：bytes -> upload；image url -> embed image :contentReference[oaicite:38]{index=38}
        let mut files: Vec<CreateAttachment> = vec![];
        let mut embeds: Vec<CreateEmbed> = vec![];

        for a in &out.attachments {
            if let Some(bytes) = &a.bytes {
                let name = a.name.clone().unwrap_or_else(|| {
                    if a.ty == OutContentType::Image {
                        "image.bin".into()
                    } else {
                        "file.bin".into()
                    }
                });
                files.push(CreateAttachment::bytes((**bytes).clone(), name));
            } else if a.ty == OutContentType::Image {
                if let Some(url) = &a.url {
                    if !url.trim().is_empty() {
                        embeds.push(CreateEmbed::new().image(url));
                    }
                }
            }
        }

        let content = out.text.clone().unwrap_or_default();

        if out.addr.is_group {
            let ch = ChannelId::new(out.addr.chat_id as u64);
            self.send_to_channel(ch, content, embeds, files).await
        } else {
            let uid = UserId::new(out.addr.chat_id as u64);
            let dm = uid.create_dm_channel(&*self.http).await?;
            self.send_to_channel(dm.id, content, embeds, files).await
        }
    }
}
