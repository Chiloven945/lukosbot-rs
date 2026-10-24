/// Which built-in commands are registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BasicCommandsConfig {
    pub ping: bool,
    pub echo: bool,
    pub coin: bool,
    pub dice: bool,
    pub luck: bool,
    pub twenty_four: bool,
    pub weather: bool,
}

impl Default for BasicCommandsConfig {
    fn default() -> Self {
        Self {
            ping: true,
            echo: true,
            coin: true,
            dice: true,
            luck: true,
            twenty_four: true,
            weather: true,
        }
    }
}

/// Which third-party integration commands are registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThirdPartyCommandsConfig {
    pub github: bool,
    pub bilibili: bool,
    pub ip: bool,
    pub music: bool,
    pub whois: bool,
}

impl ThirdPartyCommandsConfig {
    pub const fn all_disabled() -> Self {
        Self {
            github: false,
            bilibili: false,
            ip: false,
            music: false,
            whois: false,
        }
    }
}

impl Default for ThirdPartyCommandsConfig {
    fn default() -> Self {
        Self {
            github: true,
            bilibili: true,
            ip: true,
            music: true,
            whois: true,
        }
    }
}

/// Which media commands are registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaCommandsConfig {
    pub cave: bool,
    pub e621: bool,
}

impl MediaCommandsConfig {
    pub const fn all_disabled() -> Self {
        Self {
            cave: false,
            e621: false,
        }
    }
}

impl Default for MediaCommandsConfig {
    fn default() -> Self {
        Self {
            cave: true,
            e621: true,
        }
    }
}

/// Which web commands are registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebCommandsConfig {
    pub wiki: bool,
    pub mcwiki: bool,
}

impl WebCommandsConfig {
    pub const fn all_disabled() -> Self {
        Self {
            wiki: false,
            mcwiki: false,
        }
    }
}

impl Default for WebCommandsConfig {
    fn default() -> Self {
        Self {
            wiki: true,
            mcwiki: true,
        }
    }
}
