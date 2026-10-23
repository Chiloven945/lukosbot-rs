use async_trait::async_trait;

use crate::context::CommandContext;
use crate::error::CommandError;
use crate::invocation::CommandInvocation;

/// The executable body of one command node.
///
/// Executors are shared for the lifetime of the registry; per-message state belongs in the
/// invocation or in an explicitly shared service.
#[async_trait]
pub trait CommandExecutor: Send + Sync {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError>;
}
