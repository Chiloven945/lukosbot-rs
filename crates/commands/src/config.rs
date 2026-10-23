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
