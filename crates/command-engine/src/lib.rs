//! Chat command definitions, parsing, registration, execution, and text help.
//!
//! The engine is deliberately independent of any chat platform SDK. Commands receive a
//! [`CommandContext`] backed by the core outbound handle and state store, so they can be tested
//! without Telegram or Discord credentials.

#![forbid(unsafe_code)]

pub mod args;
pub mod context;
pub mod error;
pub mod executor;
pub mod handler;
pub mod help;
pub mod invocation;
pub mod lexer;
pub mod name;
pub mod parser;
pub mod registry;
pub mod spec;

pub use args::{ArgAccessError, ArgType, ArgValue, FromArgValue, ParsedArgs, Validator};
pub use context::CommandContext;
pub use error::{
    CommandError, CommandNameError, CommandParseError, CommandRegistryError, ResolveError,
};
pub use executor::CommandExecutor;
pub use handler::{CommandHandlerError, CommandMessageHandler};
pub use invocation::CommandInvocation;
pub use lexer::{LexError, Token};
pub use name::CommandName;
pub use parser::parse_args;
pub use registry::{CommandRegistry, CommandRegistryBuilder, ResolvedCommand};
pub use spec::{
    ArgsSpec, CommandDefinition, CommandNode, InputSpec, OptionSpec, PositionalSpec, SyntaxLine,
    SyntaxPart, UsageMetadata,
};
