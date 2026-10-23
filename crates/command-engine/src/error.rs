use thiserror::Error;

use lukosbot_core::error::BoxError;
use lukosbot_core::messaging::outbound::SendError;

pub use crate::name::CommandNameError;

/// An input error discovered while parsing command arguments.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CommandParseError {
    #[error("引号未闭合")]
    UnterminatedQuote { position: usize },

    #[error("未知参数：{name}")]
    UnknownOption { name: String },

    #[error("参数 {name} 需要一个值")]
    MissingOptionValue { name: String },

    #[error("缺少必填参数：{name}")]
    MissingArgument { name: String },

    #[error("参数过多，无法解析：{rest}")]
    TooManyArguments { rest: String },

    #[error("不需要参数，但收到了：{rest}")]
    UnexpectedArguments { rest: String },

    #[error("参数值无效：{value}，可选值：{choices}")]
    InvalidChoice { value: String, choices: String },

    #[error("参数需要整数，收到：{value}")]
    InvalidInteger { value: String },

    #[error("参数值无效：{value}")]
    InvalidValue { value: String },

    #[error("{message}")]
    Validation { name: String, message: String },

    #[error("参数 {name} 的值不能为空")]
    EmptyValue { name: String },
}

/// A command definition or registration conflict.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CommandRegistryError {
    #[error(transparent)]
    Name(#[from] CommandNameError),

    #[error("duplicate command name or alias: {name}")]
    DuplicateName { name: String },

    #[error("command `{path}` has an invalid definition: {message}")]
    InvalidDefinition { path: String, message: String },
}

/// A failure while resolving a command path.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ResolveError {
    #[error("未知子命令：{token}")]
    UnknownSubcommand { path: String, token: String },

    #[error("缺少子命令")]
    MissingSubcommand { path: String },
}

/// A failure returned by a command executor.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CommandError {
    /// A user-facing business validation error.
    #[error("{0}")]
    UserInput(String),

    /// The reply could not be delivered.
    #[error(transparent)]
    Send(#[from] SendError),

    /// An unexpected internal failure.
    #[error("command execution failed")]
    Internal(#[source] BoxError),
}

impl CommandError {
    pub fn internal(error: impl Into<BoxError>) -> Self {
        Self::Internal(error.into())
    }
}
