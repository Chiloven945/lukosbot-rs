use anyhow::{anyhow, Result};

use crate::core::command_source::CommandSource;

pub use azalea_brigadier::prelude::*;

pub use azalea_brigadier::context::CommandContext;

pub type CommandDispatcher<S> = azalea_brigadier::command_dispatcher::CommandDispatcher<S>;

pub struct CommandDispatcherWrapper {
    dispatcher: CommandDispatcher<CommandSource>,
}

impl CommandDispatcherWrapper {
    pub fn new() -> Self {
        let mut dispatcher = CommandDispatcher::<CommandSource>::new();

        dispatcher.register(
            literal("ping").executes(|ctx: &CommandContext<CommandSource>| {
                // ctx.source: Arc<CommandSource>
                ctx.source.reply("PONG");
                1
            }),
        );

        Self { dispatcher }
    }

    pub fn execute(&self, cmd_line: &str, src: CommandSource) -> Result<i32> {
        self.dispatcher
            .execute(cmd_line, src)
            .map_err(|e| anyhow!("Command execution failed: {}", e.message()))
    }
}
