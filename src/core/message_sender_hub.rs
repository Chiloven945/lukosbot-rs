use anyhow::Result;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tracing::{error, info, warn};

use crate::model::{Attachment, ChatPlatform, MessageOut};

#[async_trait]
pub trait Sender: Send + Sync {
    async fn send(&self, out: MessageOut) -> Result<()>;
}

#[derive(Clone)]
pub struct MessageSenderHub {
    senders: Arc<Mutex<HashMap<ChatPlatform, Arc<dyn Sender>>>>,
}

impl MessageSenderHub {
    pub fn new() -> Self {
        Self {
            senders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, p: ChatPlatform, s: Arc<dyn Sender>) {
        self.senders.lock().unwrap().insert(p, s);
    }

    pub async fn send_batch(&self, outs: Vec<MessageOut>, preserve_order: bool) {
        if preserve_order {
            for o in outs {
                let _ = self.send_one(o).await;
            }
        } else {
            // 简单并发（Java 的 false 走并发 lane）&#8203;:contentReference[oaicite:23]{index=23}
            let mut tasks = vec![];
            for o in outs {
                let hub = self.clone();
                tasks.push(tokio::spawn(async move {
                    let _ = hub.send_one(o).await;
                }));
            }
            for t in tasks {
                let _ = t.await;
            }
        }
    }

    async fn send_one(&self, out: MessageOut) -> Result<()> {
        let att = out.attachments.len();
        let text = out.text.as_deref().unwrap_or("");

        info!(
            "OUT -> [{:?}] to chat={} text=\"{}\" attachments={}",
            out.addr.platform, out.addr.chat_id, text, att
        );

        let p = out.addr.platform;
        let s = { self.senders.lock().unwrap().get(&p).cloned() };

        let Some(sender) = s else {
            warn!("No Sender for platform: {:?}", p);
            return Ok(());
        };

        if let Err(e) = sender.send(out).await {
            error!("Send failed on platform {:?}: {:?}", p, e);
        }

        Ok(())
    }
}
