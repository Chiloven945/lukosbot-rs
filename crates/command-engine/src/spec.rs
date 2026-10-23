use std::sync::Arc;

use crate::args::{ArgType, ArgValue, Validator};
use crate::executor::CommandExecutor;

/// How a command node consumes the text after its literal path.
#[derive(Debug, Clone)]
pub enum InputSpec {
    /// The command accepts no arguments at all.
    NoArgs,

    /// The command receives the untouched remainder of the command line.
    Raw { name: String, required: bool },

    /// The command receives structured positional arguments and options.
    Args(ArgsSpec),
}

#[derive(Debug, Clone, Default)]
pub struct ArgsSpec {
    pub positionals: Vec<PositionalSpec>,
    pub options: Vec<OptionSpec>,
}

#[derive(Debug, Clone)]
pub struct PositionalSpec {
    pub name: String,
    pub arg_type: ArgType,
    pub required: bool,
    pub default: Option<ArgValue>,
    pub greedy: bool,
    pub description: String,
    pub validator: Option<Validator>,
}

#[derive(Debug, Clone)]
pub struct OptionSpec {
    pub canonical_name: String,
    pub names: Vec<String>,
    pub arg_type: ArgType,
    pub required: bool,
    pub default: Option<ArgValue>,
    pub repeatable: bool,
    pub split_by: Option<String>,
    pub description: String,
    pub validator: Option<Validator>,
}

impl PositionalSpec {
    pub fn new(name: impl Into<String>, arg_type: ArgType) -> Self {
        Self {
            name: name.into(),
            arg_type,
            required: true,
            default: None,
            greedy: false,
            description: String::new(),
            validator: None,
        }
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn default(mut self, value: ArgValue) -> Self {
        self.default = Some(value);
        self.required = false;
        self
    }

    pub fn greedy(mut self) -> Self {
        self.greedy = true;
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn validator(mut self, validator: Validator) -> Self {
        self.validator = Some(validator);
        self
    }
}

impl OptionSpec {
    pub fn new(
        canonical_name: impl Into<String>,
        names: impl IntoIterator<Item = impl Into<String>>,
        arg_type: ArgType,
    ) -> Self {
        Self {
            canonical_name: canonical_name.into(),
            names: names.into_iter().map(Into::into).collect(),
            arg_type,
            required: false,
            default: None,
            repeatable: false,
            split_by: None,
            description: String::new(),
            validator: None,
        }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn default(mut self, value: ArgValue) -> Self {
        self.default = Some(value);
        self
    }

    pub fn repeatable(mut self) -> Self {
        self.repeatable = true;
        self
    }

    pub fn split_by(mut self, separator: impl Into<String>) -> Self {
        self.split_by = Some(separator.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn validator(mut self, validator: Validator) -> Self {
        self.validator = Some(validator);
        self
    }
}

/// Documentation attached to one command node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageMetadata {
    pub syntaxes: Vec<SyntaxLine>,
    pub examples: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxLine {
    pub description: String,
    pub parts: Vec<SyntaxPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxPart {
    Literal(String),
    Argument {
        name: String,
        optional: bool,
    },
    Option {
        names: Vec<String>,
        value_name: Option<String>,
        optional: bool,
    },
    OneOf(Vec<SyntaxPart>),
}

impl SyntaxLine {
    pub fn new(parts: impl IntoIterator<Item = SyntaxPart>) -> Self {
        Self {
            description: String::new(),
            parts: parts.into_iter().collect(),
        }
    }

    pub fn described(
        description: impl Into<String>,
        parts: impl IntoIterator<Item = SyntaxPart>,
    ) -> Self {
        Self {
            description: description.into(),
            parts: parts.into_iter().collect(),
        }
    }
}

/// A complete top-level command.
pub struct CommandDefinition {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub visible: bool,
    pub usage: UsageMetadata,
    pub root: CommandNode,
}

impl CommandDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input: InputSpec,
        executor: Arc<dyn CommandExecutor>,
    ) -> Self {
        let name = name.into();
        let description = description.into();
        Self {
            name: name.clone(),
            aliases: Vec::new(),
            description: description.clone(),
            visible: true,
            usage: UsageMetadata::default(),
            root: CommandNode {
                name,
                aliases: Vec::new(),
                description,
                input,
                executor: Some(executor),
                children: Vec::new(),
                usage: UsageMetadata::default(),
            },
        }
    }

    /// Creates a top-level command that only dispatches to child literals.
    pub fn group(name: impl Into<String>, description: impl Into<String>) -> Self {
        let name = name.into();
        let description = description.into();
        Self {
            name: name.clone(),
            aliases: Vec::new(),
            description: description.clone(),
            visible: true,
            usage: UsageMetadata::default(),
            root: CommandNode {
                name,
                aliases: Vec::new(),
                description,
                input: InputSpec::NoArgs,
                executor: None,
                children: Vec::new(),
                usage: UsageMetadata::default(),
            },
        }
    }

    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    pub fn aliases(mut self, aliases: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.aliases.extend(aliases.into_iter().map(Into::into));
        self
    }

    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }

    pub fn usage(mut self, usage: UsageMetadata) -> Self {
        self.usage = usage;
        self
    }

    pub fn child(mut self, child: CommandNode) -> Self {
        self.root.children.push(child);
        self
    }
}

/// One literal node in a command path.
pub struct CommandNode {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub input: InputSpec,
    pub executor: Option<Arc<dyn CommandExecutor>>,
    pub children: Vec<CommandNode>,
    pub usage: UsageMetadata,
}

impl CommandNode {
    pub fn group(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            description: description.into(),
            input: InputSpec::NoArgs,
            executor: None,
            children: Vec::new(),
            usage: UsageMetadata::default(),
        }
    }

    pub fn leaf(
        name: impl Into<String>,
        description: impl Into<String>,
        input: InputSpec,
        executor: Arc<dyn CommandExecutor>,
    ) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            description: description.into(),
            input,
            executor: Some(executor),
            children: Vec::new(),
            usage: UsageMetadata::default(),
        }
    }

    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    pub fn aliases(mut self, aliases: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.aliases.extend(aliases.into_iter().map(Into::into));
        self
    }

    pub fn usage(mut self, usage: UsageMetadata) -> Self {
        self.usage = usage;
        self
    }

    pub fn child(mut self, child: CommandNode) -> Self {
        self.children.push(child);
        self
    }
}
