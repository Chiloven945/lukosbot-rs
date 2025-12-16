use anyhow::{anyhow, Result};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use teloxide::dispatching::DefaultKey;
use teloxide::{dispatching::UpdateFilterExt, prelude::*};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::core::message_sender_hub::Sender;
use crate::lifecycle::Closeable;
use crate::model::{Address, ChatPlatform, MessageIn};

use super::sender::TelegramSender;

pub type InSink = mpsc::UnboundedSender<MessageIn>;

struct TelegramStack {
    bot: Bot,
}

pub struct TelegramReceiver {
    stack: Arc<TelegramStack>,
    sink: Arc<Mutex<Option<InSink>>>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl TelegramReceiver {
    pub fn new(token: String) -> Self {
        let bot = Bot::new(token);
        Self {
            stack: Arc::new(TelegramStack { bot }),
            sink: Arc::new(Mutex::new(None)),
            task: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn bind(&self, sink: InSink) {
        *self.sink.lock().unwrap() = Some(sink);
    }

    pub async fn start(&self) -> Result<()> {
        if self.task.lock().unwrap().is_some() {
            return Ok(());
        }

        let sink = self
            .sink
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("TelegramReceiver.start() called before bind()"))?;

        let bot = self.stack.bot.clone();

        let handler = teloxide::dptree::entry().branch(Update::filter_message().endpoint(
            move |msg: Message| {
                let sink = sink.clone();
                async move {
                    if let Some(text) = msg.text() {
                        let chat_id = msg.chat.id.0;
                        let is_group = msg.chat.is_group() || msg.chat.is_supergroup();

                        let user_id = msg.from.as_ref().map(|u| u.id.0 as i64);

                        let _ = sink.send(MessageIn::new(
                            Address::new(ChatPlatform::Telegram, chat_id, is_group),
                            user_id,
                            text.to_string(),
                        ));
                    }

                    Ok::<(), Infallible>(())
                }
            },
        ));

        let mut dispatcher =
            Dispatcher::<Bot, Infallible, DefaultKey>::builder(bot, handler).build();

        let jh = tokio::spawn(async move {
            dispatcher.dispatch().await;
        });

        *self.task.lock().unwrap() = Some(jh);
        Ok(())
    }

    pub async fn sender(&self) -> Result<Arc<dyn Sender>> {
        self.start().await?;
        Ok(Arc::new(TelegramSender::new(self.stack.bot.clone())))
    }
}

impl Closeable for TelegramReceiver {
    fn close(&self) {
        if let Some(jh) = self.task.lock().unwrap().take() {
            jh.abort();
        }
    }
}
