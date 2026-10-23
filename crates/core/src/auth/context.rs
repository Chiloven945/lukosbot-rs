/// Authorization snapshot for one actor in one message context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthContext {
    pub bot_admin: bool,
    pub chat_admin: bool,
}

impl AuthContext {
    pub const fn can_manage_global(self) -> bool {
        self.bot_admin
    }

    pub const fn can_manage_chat(self) -> bool {
        self.bot_admin || self.chat_admin
    }
}
