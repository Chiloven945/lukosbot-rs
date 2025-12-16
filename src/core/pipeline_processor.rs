use std::sync::Arc;

use crate::config::AppProperties;
use crate::core::command_processor::CommandProcessor;
use crate::core::command_registry::CommandRegistry;
use crate::model::{MessageIn, MessageOut};

pub struct PipelineProcessor {
    cmd: CommandProcessor,
}

impl PipelineProcessor {
    pub fn new(props: Arc<AppProperties>, registry: Arc<CommandRegistry>) -> Self {
        Self {
            cmd: CommandProcessor::new(props, registry),
        }
    }

    pub fn handle(&self, input: MessageIn) -> Vec<MessageOut> {
        self.cmd.handle(input)
    }
}
