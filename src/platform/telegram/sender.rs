use anyhow::Result;
use async_trait::async_trait;
use teloxide::{prelude::*, types::InputFile};
use url::Url;

use crate::core::message_sender_hub::Sender;
use crate::model::{MessageOut, OutContentType};

#[derive(Clone)]
pub struct TelegramSender {
    bot: Bot,
}

impl TelegramSender {
    pub fn new(bot: Bot) -> Self {
        Self { bot }
    }
}

#[async_trait]
impl Sender for TelegramSender {
    async fn send(&self, out: MessageOut) -> Result<()> {
        let chat = ChatId(out.addr.chat_id);

        if let Some(text) = &out.text {
            if !text.is_empty() {
                self.bot.send_message(chat, text.clone()).await?;
            }
        }

        for a in out.attachments {
            match (a.ty, a.bytes, a.url) {
                (OutContentType::Image, Some(bytes), _) => {
                    let f = InputFile::memory((*bytes).clone())
                        .file_name(a.name.unwrap_or_else(|| "image.bin".into()));
                    self.bot.send_photo(chat, f).await?;
                }
                (OutContentType::Image, None, Some(url)) => {
                    let f = InputFile::url(Url::parse(&url)?);
                    self.bot.send_photo(chat, f).await?;
                }
                (OutContentType::File, Some(bytes), _) => {
                    let f = InputFile::memory((*bytes).clone())
                        .file_name(a.name.unwrap_or_else(|| "file.bin".into()));
                    self.bot.send_document(chat, f).await?;
                }
                (OutContentType::File, None, Some(url)) => {
                    let f = InputFile::url(Url::parse(&url)?);
                    self.bot.send_document(chat, f).await?;
                }
                _ => {}
            }
        }

        Ok(())
    }
}
