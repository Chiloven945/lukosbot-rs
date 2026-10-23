use std::collections::HashMap;
use std::sync::Arc;

use crate::args::{ArgType, ArgValue, Validator};
use crate::error::{CommandRegistryError, ResolveError};
use crate::name::{CommandName, validate_name};
use crate::spec::{ArgsSpec, CommandDefinition, CommandNode, InputSpec};

/// A command resolved to a concrete node and the remaining text.
pub struct ResolvedCommand<'a> {
    pub definition: &'a CommandDefinition,
    pub node: &'a CommandNode,
    pub path: Vec<&'a str>,
    pub tail: &'a str,
}

/// An immutable command registry.
pub struct CommandRegistry {
    definitions: Vec<Arc<CommandDefinition>>,
    index: HashMap<CommandName, usize>,
}

impl CommandRegistry {
    pub fn builder() -> CommandRegistryBuilder {
        CommandRegistryBuilder::default()
    }

    pub fn find(&self, name: &str) -> Option<&CommandDefinition> {
        let name = CommandName::parse(name).ok()?;
        let index = *self.index.get(&name)?;
        self.definitions.get(index).map(AsRef::as_ref)
    }

    pub fn commands(&self) -> impl Iterator<Item = &CommandDefinition> {
        self.definitions.iter().map(AsRef::as_ref)
    }

    pub fn visible_commands(&self) -> impl Iterator<Item = &CommandDefinition> {
        self.commands().filter(|command| command.visible)
    }

    pub fn resolve<'a>(
        &'a self,
        line: &'a str,
    ) -> Result<Option<ResolvedCommand<'a>>, ResolveError> {
        let line = line.trim_start();
        if line.is_empty() {
            return Ok(None);
        }

        let (root_token, mut rest) = split_word(line);
        let Some(root_name) = CommandName::parse(root_token).ok() else {
            return Ok(None);
        };
        let Some(index) = self.index.get(&root_name).copied() else {
            return Ok(None);
        };
        let definition = self.definitions[index].as_ref();
        let mut node = &definition.root;
        let mut path = vec![definition.name.as_str()];

        loop {
            let (next_token, after) = split_word(rest);
            if next_token.is_empty() {
                break;
            }

            if let Some(child) = node
                .children
                .iter()
                .find(|child| child_matches(child, next_token))
            {
                path.push(child.name.as_str());
                node = child;
                rest = after;
                continue;
            }

            if node.executor.is_none() && !node.children.is_empty() {
                return Err(ResolveError::UnknownSubcommand {
                    path: path.join(" "),
                    token: next_token.to_owned(),
                });
            }

            break;
        }

        if node.executor.is_none() {
            return Err(if rest.is_empty() {
                ResolveError::MissingSubcommand {
                    path: path.join(" "),
                }
            } else {
                let (token, _) = split_word(rest);
                ResolveError::UnknownSubcommand {
                    path: path.join(" "),
                    token: token.to_owned(),
                }
            });
        }

        Ok(Some(ResolvedCommand {
            definition,
            node,
            path,
            tail: rest,
        }))
    }
}

fn child_matches(child: &CommandNode, token: &str) -> bool {
    child.name.eq_ignore_ascii_case(token)
        || child
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(token))
}

fn split_word(input: &str) -> (&str, &str) {
    let input = input.trim_start();
    match input.find(char::is_whitespace) {
        Some(index) => (&input[..index], input[index..].trim_start()),
        None => (input, ""),
    }
}

/// Builds a validated, immutable [`CommandRegistry`].
#[derive(Default)]
pub struct CommandRegistryBuilder {
    definitions: Vec<CommandDefinition>,
    index: HashMap<CommandName, usize>,
}

impl CommandRegistryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, definition: CommandDefinition) -> Result<(), CommandRegistryError> {
        let root_name = CommandName::parse(&definition.name)?;
        let mut keys = vec![root_name];
        for alias in &definition.aliases {
            keys.push(CommandName::parse(alias)?);
        }

        let mut local = HashMap::new();
        for key in &keys {
            if local.insert(key.clone(), ()).is_some() {
                return Err(CommandRegistryError::DuplicateName {
                    name: key.to_string(),
                });
            }
        }

        for key in &keys {
            if self.index.contains_key(key) {
                return Err(CommandRegistryError::DuplicateName {
                    name: key.to_string(),
                });
            }
        }

        validate_definition(&definition)?;

        let command_index = self.definitions.len();
        for key in keys {
            self.index.insert(key, command_index);
        }
        self.definitions.push(definition);
        Ok(())
    }

    pub fn build(self) -> CommandRegistry {
        CommandRegistry {
            definitions: self.definitions.into_iter().map(Arc::new).collect(),
            index: self.index,
        }
    }
}

fn validate_definition(definition: &CommandDefinition) -> Result<(), CommandRegistryError> {
    validate_name(&definition.name)?;

    if !definition.root.name.eq_ignore_ascii_case(&definition.name) {
        return Err(CommandRegistryError::InvalidDefinition {
            path: definition.name.clone(),
            message: "root node name must match the command name".to_owned(),
        });
    }

    if !definition.root.aliases.is_empty() {
        return Err(CommandRegistryError::InvalidDefinition {
            path: definition.name.clone(),
            message: "declare top-level aliases on CommandDefinition, not on the root node"
                .to_owned(),
        });
    }

    validate_node(&definition.root, &definition.name)
}

fn validate_node(node: &CommandNode, path: &str) -> Result<(), CommandRegistryError> {
    validate_name(&node.name)?;
    let mut names = HashMap::new();
    names.insert(CommandName::parse(&node.name)?, ());
    for alias in &node.aliases {
        if names.insert(CommandName::parse(alias)?, ()).is_some() {
            return Err(CommandRegistryError::InvalidDefinition {
                path: path.to_owned(),
                message: format!("duplicate node alias `{alias}`"),
            });
        }
    }

    if node.executor.is_none() && node.children.is_empty() {
        return Err(CommandRegistryError::InvalidDefinition {
            path: path.to_owned(),
            message: "a node without an executor must have children".to_owned(),
        });
    }

    validate_input(&node.input, path)?;

    let mut child_names: HashMap<CommandName, ()> = HashMap::new();
    for child in &node.children {
        let mut child_keys = vec![CommandName::parse(&child.name)?];
        for alias in &child.aliases {
            child_keys.push(CommandName::parse(alias)?);
        }
        for key in child_keys {
            if child_names.insert(key.clone(), ()).is_some() {
                return Err(CommandRegistryError::InvalidDefinition {
                    path: path.to_owned(),
                    message: format!("duplicate child command name or alias `{key}`"),
                });
            }
        }

        let child_path = format!("{path} {}", child.name);
        validate_node(child, &child_path)?;
    }

    Ok(())
}

fn validate_input(input: &InputSpec, path: &str) -> Result<(), CommandRegistryError> {
    match input {
        InputSpec::NoArgs => Ok(()),
        InputSpec::Raw { name, .. } => validate_field_name(name, path),
        InputSpec::Args(args) => validate_args(args, path),
    }
}

fn validate_args(args: &ArgsSpec, path: &str) -> Result<(), CommandRegistryError> {
    let mut field_names = HashMap::new();
    let mut option_names = HashMap::new();
    let mut greedy_seen = false;

    for (index, positional) in args.positionals.iter().enumerate() {
        validate_field_name(&positional.name, path)?;
        if field_names.insert(positional.name.clone(), ()).is_some() {
            return Err(invalid(
                path,
                format!("duplicate argument name `{}`", positional.name),
            ));
        }
        validate_arg_type(&positional.arg_type, path)?;
        if positional.required && positional.default.is_some() {
            return Err(invalid(
                path,
                format!(
                    "required positional `{}` cannot have a default",
                    positional.name
                ),
            ));
        }
        if positional.greedy {
            if greedy_seen || index + 1 != args.positionals.len() {
                return Err(invalid(
                    path,
                    "a greedy positional must be unique and last".to_owned(),
                ));
            }
            greedy_seen = true;
        }
        if let Some(default) = &positional.default {
            validate_default(
                &positional.arg_type,
                default,
                &positional.name,
                positional.validator.as_ref(),
                false,
                path,
            )?;
        }
    }

    for option in &args.options {
        validate_field_name(&option.canonical_name, path)?;
        if field_names
            .insert(option.canonical_name.clone(), ())
            .is_some()
        {
            return Err(invalid(
                path,
                format!("duplicate argument name `{}`", option.canonical_name),
            ));
        }
        if option.names.is_empty() {
            return Err(invalid(
                path,
                format!("option `{}` has no names", option.canonical_name),
            ));
        }
        for name in &option.names {
            validate_option_name(name, path)?;
            if option_names.insert(name.clone(), ()).is_some() {
                return Err(invalid(path, format!("duplicate option name `{name}`")));
            }
        }
        if option
            .split_by
            .as_deref()
            .is_some_and(|separator| separator.is_empty())
        {
            return Err(invalid(
                path,
                format!("option `{}` has an empty split_by", option.canonical_name),
            ));
        }
        validate_arg_type(&option.arg_type, path)?;
        if option.required && option.default.is_some() {
            return Err(invalid(
                path,
                format!(
                    "required option `{}` cannot have a default",
                    option.canonical_name
                ),
            ));
        }
        if let Some(default) = &option.default {
            let expects_list = option.repeatable || option.split_by.is_some();
            validate_default(
                &option.arg_type,
                default,
                &option.canonical_name,
                option.validator.as_ref(),
                expects_list,
                path,
            )?;
        }
    }

    Ok(())
}

fn validate_field_name(name: &str, path: &str) -> Result<(), CommandRegistryError> {
    if name.is_empty()
        || name.chars().any(char::is_whitespace)
        || name.chars().any(char::is_control)
    {
        return Err(invalid(path, format!("invalid argument name `{name}`")));
    }
    Ok(())
}

fn validate_option_name(name: &str, path: &str) -> Result<(), CommandRegistryError> {
    let valid_long = name.starts_with("--") && name.len() > 2;
    let valid_short = name.starts_with('-') && !name.starts_with("--") && name.chars().count() == 2;
    if (!valid_long && !valid_short)
        || name.chars().any(char::is_whitespace)
        || name.chars().any(char::is_control)
    {
        return Err(invalid(path, format!("invalid option name `{name}`")));
    }
    Ok(())
}

fn validate_arg_type(arg_type: &ArgType, path: &str) -> Result<(), CommandRegistryError> {
    if let ArgType::Choice(choices) = arg_type {
        if choices.is_empty() {
            return Err(invalid(
                path,
                "choice types must declare at least one value".to_owned(),
            ));
        }
        let mut seen = HashMap::new();
        for choice in choices {
            if seen.insert(choice.clone(), ()).is_some() {
                return Err(invalid(path, format!("duplicate choice `{choice}`")));
            }
        }
    }
    Ok(())
}

fn validate_default(
    arg_type: &ArgType,
    default: &ArgValue,
    name: &str,
    validator: Option<&Validator>,
    expects_list: bool,
    path: &str,
) -> Result<(), CommandRegistryError> {
    if expects_list {
        let ArgValue::List(values) = default else {
            return Err(invalid(
                path,
                format!("default for `{name}` must be a list"),
            ));
        };
        if values.is_empty() {
            return Err(invalid(
                path,
                format!("default for `{name}` must not be empty"),
            ));
        }
        for value in values {
            validate_default_value(arg_type, value, name, validator, path)?;
        }
    } else {
        if matches!(default, ArgValue::List(_)) {
            return Err(invalid(
                path,
                format!("default for `{name}` must be a scalar"),
            ));
        }
        validate_default_value(arg_type, default, name, validator, path)?;
    }
    Ok(())
}

fn validate_default_value(
    arg_type: &ArgType,
    value: &ArgValue,
    name: &str,
    validator: Option<&Validator>,
    path: &str,
) -> Result<(), CommandRegistryError> {
    if !arg_type.accepts(value) {
        return Err(invalid(
            path,
            format!(
                "default for `{name}` has type {}, expected {}",
                value.type_name(),
                arg_type.name()
            ),
        ));
    }
    if let Some(validator) = validator {
        validator.validate(value).map_err(|message| {
            invalid(path, format!("default for `{name}` is invalid: {message}"))
        })?;
    }
    Ok(())
}

fn invalid(path: &str, message: String) -> CommandRegistryError {
    CommandRegistryError::InvalidDefinition {
        path: path.to_owned(),
        message,
    }
}
