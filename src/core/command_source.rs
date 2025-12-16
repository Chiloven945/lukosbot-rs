use std::sync::{Arc, Mutex};

use crate::model::{Attachment, MessageIn, MessageOut};

#[derive(Clone)]
pub struct CommandSource {
    in_msg: MessageIn,
    outs: Arc<Mutex<Vec<MessageOut>>>,
}

impl CommandSource {
    pub fn new(in_msg: MessageIn) -> Self {
        Self {
            in_msg,
            outs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn in_msg(&self) -> &MessageIn {
        &self.in_msg
    }

    pub fn reply(&self, text: impl Into<String>) {
        self.outs
            .lock()
            .unwrap()
            .push(MessageOut::text(self.in_msg.addr.clone(), text));
    }

    pub fn reply_out(&self, out: MessageOut) {
        self.outs.lock().unwrap().push(out);
    }

    pub fn reply_image_url(&self, url: impl Into<String>) {
        self.reply_out(MessageOut {
            addr: self.in_msg.addr.clone(),
            text: None,
            attachments: vec![Attachment::image_url(url)],
        });
    }

    pub fn reply_image_bytes(
        &self,
        name: impl Into<String>,
        bytes: Vec<u8>,
        mime: impl Into<String>,
    ) {
        self.reply_out(MessageOut {
            addr: self.in_msg.addr.clone(),
            text: None,
            attachments: vec![Attachment::image_bytes(name, bytes, mime)],
        });
    }

    pub fn reply_file_url(&self, name: impl Into<String>, url: impl Into<String>) {
        self.reply_out(MessageOut {
            addr: self.in_msg.addr.clone(),
            text: None,
            attachments: vec![Attachment::file_url(name, url)],
        });
    }

    pub fn reply_file_bytes(
        &self,
        name: impl Into<String>,
        bytes: Vec<u8>,
        mime: impl Into<String>,
    ) {
        self.reply_out(MessageOut {
            addr: self.in_msg.addr.clone(),
            text: None,
            attachments: vec![Attachment::file_bytes(name, bytes, mime)],
        });
    }

    pub fn take_outs(&self) -> Vec<MessageOut> {
        std::mem::take(&mut *self.outs.lock().unwrap())
    }
}
