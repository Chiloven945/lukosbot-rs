use crate::core::command_registry::BotCommand;
use crate::core::command_source::CommandSource;
use crate::core::dispatcher::{literal, CommandContext, CommandDispatcher};

pub struct PingCommand;

impl BotCommand for PingCommand {
    fn name(&self) -> &'static str {
        "ping"
    }
    fn description(&self) -> &'static str {
        "Ping command"
    }
    fn usage(&self) -> &'static str {
        "ping"
    }

    fn register(&self, d: &mut CommandDispatcher<CommandSource>) {
        d.register(
            literal("ping").executes(|ctx: &CommandContext<CommandSource>| {
                ctx.source.reply("PONG");
                1
            }),
        );
    }
}
