use crate::args::ParsedArgs;

/// One resolved command invocation.
#[derive(Debug, Clone)]
pub struct CommandInvocation {
    /// The command line with the global prefix removed and outer whitespace trimmed.
    pub raw_command_line: String,

    /// The canonical command path, for example `["help"]` or `["music", "link"]`.
    pub path: Vec<String>,

    /// The untouched text after the matched literal path.
    pub raw_tail: String,

    /// Values parsed for `InputSpec::Args`; empty for raw and no-args commands.
    pub args: ParsedArgs,
}
