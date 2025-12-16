use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::core::message_sender_hub::Sender;
use crate::lifecycle::Closeable;
use crate::model::MessageIn;

use super::sender::DiscordSender;
use super::stack::DiscordStack;

pub struct DiscordReceiver {
    stack: Arc<DiscordStack>,
}

impl DiscordReceiver {
    pub fn new(token: String) -> Self {
        Self { stack: DiscordStack::new(token) }
    }

    pub async fn bind(&self, sink: mpsc::UnboundedSender<MessageIn>) {
        self.stack.set_sink(sink).await;
    }

    pub async fn start(&self) -> Result<()> {
        self.stack.ensure_started().await
    }

    pub async fn sender(&self) -> Result<Arc<dyn Sender>> {
        self.start().await?;
        Ok(Arc::new(DiscordSender::new(self.stack.clone())))
    }
}

impl Closeable for DiscordReceiver {
    fn close(&self) {
    }
}