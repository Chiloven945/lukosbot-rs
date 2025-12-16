use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatPlatform {
    Telegram,
    Discord,
    Onebot,
}

#[derive(Debug, Clone)]
pub struct Address {
    pub platform: ChatPlatform,
    pub chat_id: i64,
    pub is_group: bool,
}

impl Address {
    pub fn new(platform: ChatPlatform, chat_id: i64, is_group: bool) -> Self {
        Self {
            platform,
            chat_id,
            is_group,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MessageIn {
    pub addr: Address,
    pub user_id: Option<i64>,
    pub text: String,
}

impl MessageIn {
    pub fn new(addr: Address, user_id: impl Into<Option<i64>>, text: String) -> Self {
        Self {
            addr,
            user_id: user_id.into(),
            text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutContentType {
    Image,
    File,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub ty: OutContentType,
    pub name: Option<String>,
    pub url: Option<String>,
    pub bytes: Option<Arc<Vec<u8>>>,
    pub mime: Option<String>,
}

impl Attachment {
    pub fn image_url(url: impl Into<String>) -> Self {
        Self {
            ty: OutContentType::Image,
            name: None,
            url: Some(url.into()),
            bytes: None,
            mime: None,
        }
    }

    pub fn image_bytes(name: impl Into<String>, bytes: Vec<u8>, mime: impl Into<String>) -> Self {
        Self {
            ty: OutContentType::Image,
            name: Some(name.into()),
            url: None,
            bytes: Some(Arc::new(bytes)),
            mime: Some(mime.into()),
        }
    }

    pub fn file_url(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            ty: OutContentType::File,
            name: Some(name.into()),
            url: Some(url.into()),
            bytes: None,
            mime: None,
        }
    }

    pub fn file_bytes(name: impl Into<String>, bytes: Vec<u8>, mime: impl Into<String>) -> Self {
        Self {
            ty: OutContentType::File,
            name: Some(name.into()),
            url: None,
            bytes: Some(Arc::new(bytes)),
            mime: Some(mime.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MessageOut {
    pub addr: Address,
    pub text: Option<String>,
    pub attachments: Vec<Attachment>,
}

impl MessageOut {
    pub fn text(addr: Address, text: impl Into<String>) -> Self {
        Self {
            addr,
            text: Some(text.into()),
            attachments: vec![],
        }
    }
}
