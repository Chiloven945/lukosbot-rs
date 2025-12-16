use crate::config::AppProperties;
use crate::core::command_registry::CommandRegistry;
use crate::core::command_source::CommandSource;
use crate::core::dispatcher::CommandDispatcher;
use crate::model::{MessageIn, MessageOut};
use std::sync::Arc;

pub struct CommandProcessor {
    props: Arc<AppProperties>,
    dispatcher: CommandDispatcher<CommandSource>,
    registry: Arc<CommandRegistry>,
}

impl CommandProcessor {
    pub fn new(props: Arc<AppProperties>, registry: Arc<CommandRegistry>) -> Self {
        let mut dispatcher = CommandDispatcher::<CommandSource>::new();
        for c in registry.all() {
            c.register(&mut dispatcher);
        }

        Self {
            props,
            dispatcher,
            registry,
        }
    }

    pub fn handle(&self, input: MessageIn) -> Vec<MessageOut> {
        let cmd_line: String = {
            let t = input.text.trim();
            let rest = match t.strip_prefix(&self.props.prefix) {
                Some(r) => r,
                None => return vec![],
            };
            rest.trim().to_string()
        };

        let src = CommandSource::new(input);

        if let Err(e) = self.dispatcher.execute(cmd_line.as_str(), src.clone()) {
            src.reply(format!("命令错误: {}", e.message()));
        }

        src.take_outs()
    }
}
