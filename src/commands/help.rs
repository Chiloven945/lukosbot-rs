use crate::config::AppProperties;
use crate::core::command_registry::{BotCommand, CommandRegistry};
use crate::core::command_source::CommandSource;
use crate::core::dispatcher::{literal, CommandContext, CommandDispatcher};
use std::sync::{Arc, Weak};

pub struct HelpCommand {
    registry: Weak<CommandRegistry>,
    props: Arc<AppProperties>,
}

impl HelpCommand {
    pub fn new(registry: Weak<CommandRegistry>, props: Arc<AppProperties>) -> Self {
        Self { registry, props }
    }
}

impl BotCommand for HelpCommand {
    fn name(&self) -> &'static str {
        "help"
    }

    fn description(&self) -> &'static str {
        "列出可用命令或其详细用法"
    }

    fn usage(&self) -> &'static str {
        "用法：\n`/help`           # 列出所有可用命令\n`/help <command>` # 显示指定命令的用法\n"
    }

    fn register(&self, dispatcher: &mut CommandDispatcher<CommandSource>) {
        let props = self.props.clone();
        let registry = self.registry.clone();

        dispatcher.register(literal("help").executes(
            move |ctx: &CommandContext<CommandSource>| {
                let Some(reg) = registry.upgrade() else {
                    ctx.source.reply("命令系统未初始化。");
                    return 1;
                };

                let mut sb = String::from("可用命令：\n");
                for c in reg.all().iter().filter(|c| c.visible()) {
                    sb.push_str(&format!(
                        "{}{} - {}\n",
                        props.prefix,
                        c.name(),
                        c.description()
                    ));
                }
                sb.push_str("\n使用 `/help <command>` 查看具体命令的用法。");
                ctx.source.reply(sb.trim().to_string());
                1
            },
        ));
    }
}
