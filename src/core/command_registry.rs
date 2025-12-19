use std::sync::Arc;

use crate::commands::{github::GitHubCommand, help::HelpCommand, ping::PingCommand};
use crate::config::AppProperties;
use crate::core::command_source::CommandSource;
use crate::core::dispatcher::CommandDispatcher;

pub trait BotCommand: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn usage(&self) -> &'static str;

    fn visible(&self) -> bool {
        true
    }

    fn register(&self, d: &mut CommandDispatcher<CommandSource>);
}

pub struct CommandRegistry {
    cmds: Vec<Arc<dyn BotCommand>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self { cmds: vec![] }
    }

    pub fn build(props: Arc<AppProperties>) -> Arc<Self> {
        Arc::new_cyclic(|weak_reg| {
            let help = HelpCommand::new(weak_reg.clone(), props.clone());
            let github = GitHubCommand::new(
                Option::from(props.commands.github.token.clone()),
                &props.proxy.clone(),
            );

            CommandRegistry {
                cmds: vec![
                    Arc::new(PingCommand) as Arc<dyn BotCommand>,
                    Arc::new(github) as Arc<dyn BotCommand>,
                    Arc::new(help) as Arc<dyn BotCommand>,
                ],
            }
        })
    }

    pub fn all(&self) -> &[Arc<dyn BotCommand>] {
        &self.cmds
    }

    pub fn list_commands(&self) -> String {
        self.cmds
            .iter()
            .map(|c| c.name())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
