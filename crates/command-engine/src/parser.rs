use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::args::{ArgType, ArgValue, ParsedArgs};
use crate::error::CommandParseError;
use crate::lexer::Token;
use crate::spec::{ArgsSpec, OptionSpec, PositionalSpec};

/// Parses argv-style tokens according to a command's positional and option specifications.
pub fn parse_args(tokens: &[Token], spec: &ArgsSpec) -> Result<ParsedArgs, CommandParseError> {
    let options = option_index(&spec.options);
    let mut values = HashMap::new();
    let mut raw_positionals = Vec::new();
    let mut stop_options = false;
    let mut index = 0;

    while index < tokens.len() {
        let token = &tokens[index];
        let value = token.value.as_str();

        if !stop_options && value == "--" {
            stop_options = true;
            index += 1;
            continue;
        }

        if !stop_options && value.starts_with("--") {
            index = parse_long_option(tokens, index, value, &options, &mut values)?;
            index += 1;
            continue;
        }

        if !stop_options
            && value.starts_with('-')
            && value.chars().count() == 2
            && !is_negative_number(value)
        {
            index = parse_short_option(tokens, index, value, &options, &mut values)?;
            index += 1;
            continue;
        }

        raw_positionals.push(value.to_owned());
        index += 1;
    }

    process_positionals(&mut values, &raw_positionals, &spec.positionals)?;
    apply_defaults(&mut values, &spec.positionals, &spec.options)?;

    Ok(ParsedArgs::new(values, raw_positionals))
}

fn option_index(options: &[OptionSpec]) -> HashMap<&str, &OptionSpec> {
    let mut index = HashMap::new();
    for option in options {
        for name in &option.names {
            index.insert(name.as_str(), option);
        }
    }
    index
}

fn parse_long_option(
    tokens: &[Token],
    index: usize,
    token: &str,
    options: &HashMap<&str, &OptionSpec>,
    values: &mut HashMap<String, ArgValue>,
) -> Result<usize, CommandParseError> {
    let (name, inline_value) = match token.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (token, None),
    };

    let option = options
        .get(name)
        .copied()
        .ok_or_else(|| CommandParseError::UnknownOption {
            name: name.to_owned(),
        })?;

    if option.arg_type == ArgType::Bool {
        let value = match inline_value {
            Some(value) => parse_bool(value, &option.canonical_name)?,
            None => true,
        };
        store_option(values, option, ArgValue::Bool(value));
        return Ok(index);
    }

    let raw_value = match inline_value {
        Some(value) => value,
        None => next_option_value(tokens, index, name)?,
    };

    store_converted(values, option, raw_value)?;
    Ok(if inline_value.is_some() {
        index
    } else {
        index + 1
    })
}

fn parse_short_option(
    tokens: &[Token],
    index: usize,
    token: &str,
    options: &HashMap<&str, &OptionSpec>,
    values: &mut HashMap<String, ArgValue>,
) -> Result<usize, CommandParseError> {
    let option = options
        .get(token)
        .copied()
        .ok_or_else(|| CommandParseError::UnknownOption {
            name: token.to_owned(),
        })?;

    if option.arg_type == ArgType::Bool {
        store_option(values, option, ArgValue::Bool(true));
        return Ok(index);
    }

    let raw_value = next_option_value(tokens, index, token)?;
    store_converted(values, option, raw_value)?;
    Ok(index + 1)
}

fn next_option_value<'a>(
    tokens: &'a [Token],
    index: usize,
    name: &str,
) -> Result<&'a str, CommandParseError> {
    let next = tokens.get(index + 1);
    match next {
        Some(next) if !starts_option(&next.value) => Ok(&next.value),
        _ => Err(CommandParseError::MissingOptionValue {
            name: name.to_owned(),
        }),
    }
}

fn starts_option(token: &str) -> bool {
    token == "--"
        || token.starts_with("--")
        || (token.starts_with('-') && token.chars().count() == 2 && !is_negative_number(token))
}

fn is_negative_number(token: &str) -> bool {
    let mut characters = token.chars();
    characters.next() == Some('-') && characters.all(|character| character.is_ascii_digit())
}

fn parse_bool(raw: &str, name: &str) -> Result<bool, CommandParseError> {
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(CommandParseError::Validation {
            name: name.to_owned(),
            message: format!("参数 {name} 是布尔类型，只接受 true 或 false，收到：{raw}"),
        }),
    }
}

fn store_converted(
    values: &mut HashMap<String, ArgValue>,
    option: &OptionSpec,
    raw: &str,
) -> Result<(), CommandParseError> {
    if let Some(separator) = &option.split_by {
        let parts: Vec<&str> = raw
            .split(separator.as_str())
            .filter(|part| !part.is_empty())
            .collect();
        if parts.is_empty() {
            return Err(CommandParseError::EmptyValue {
                name: option.canonical_name.clone(),
            });
        }

        let mut converted = Vec::with_capacity(parts.len());
        for part in parts {
            converted.push(convert_and_validate(option, part)?);
        }

        if option.repeatable {
            append_list(values, &option.canonical_name, converted);
        } else {
            values.insert(option.canonical_name.clone(), ArgValue::List(converted));
        }
    } else {
        let converted = convert_and_validate(option, raw)?;
        if option.repeatable {
            append_list(values, &option.canonical_name, vec![converted]);
        } else {
            store_option(values, option, converted);
        }
    }

    Ok(())
}

fn append_list(values: &mut HashMap<String, ArgValue>, name: &str, items: Vec<ArgValue>) {
    match values.entry(name.to_owned()) {
        Entry::Vacant(vacant) => {
            vacant.insert(ArgValue::List(items));
        }
        Entry::Occupied(mut occupied) => {
            if let ArgValue::List(list) = occupied.get_mut() {
                list.extend(items);
            } else {
                occupied.insert(ArgValue::List(items));
            }
        }
    }
}

fn convert_and_validate(option: &OptionSpec, raw: &str) -> Result<ArgValue, CommandParseError> {
    let value = convert_value(&option.arg_type, raw, &option.canonical_name)?;
    if let Some(validator) = &option.validator {
        validator
            .validate(&value)
            .map_err(|message| CommandParseError::Validation {
                name: option.canonical_name.clone(),
                message,
            })?;
    }
    Ok(value)
}

fn convert_value(arg_type: &ArgType, raw: &str, name: &str) -> Result<ArgValue, CommandParseError> {
    match arg_type {
        ArgType::String => Ok(ArgValue::String(raw.to_owned())),
        ArgType::I32 => {
            raw.parse::<i32>()
                .map(ArgValue::I32)
                .map_err(|_| CommandParseError::InvalidInteger {
                    value: raw.to_owned(),
                })
        }
        ArgType::I64 => {
            raw.parse::<i64>()
                .map(ArgValue::I64)
                .map_err(|_| CommandParseError::InvalidInteger {
                    value: raw.to_owned(),
                })
        }
        ArgType::Bool => parse_bool(raw, name).map(ArgValue::Bool),
        ArgType::Choice(choices) => {
            if choices.iter().any(|choice| choice == raw) {
                Ok(ArgValue::String(raw.to_owned()))
            } else {
                Err(CommandParseError::InvalidChoice {
                    value: raw.to_owned(),
                    choices: choices.join(", "),
                })
            }
        }
    }
}

fn process_positionals(
    values: &mut HashMap<String, ArgValue>,
    raw_positionals: &[String],
    specs: &[PositionalSpec],
) -> Result<(), CommandParseError> {
    let mut position_index = 0;
    let mut spec_index = 0;

    while spec_index < specs.len() && position_index < raw_positionals.len() {
        let spec = &specs[spec_index];

        // An optional positional before required positionals is skipped when the remaining
        // tokens are only enough to satisfy those required arguments. This makes
        // `[scope?] <state>` accept both `get <state>` and `get <scope> <state>`.
        if !spec.required && !spec.greedy {
            let required_after = specs[spec_index + 1..]
                .iter()
                .filter(|later| later.required)
                .count();
            if raw_positionals.len() - position_index <= required_after {
                spec_index += 1;
                continue;
            }
        }

        if spec.greedy {
            let joined = raw_positionals[position_index..].join(" ");
            let value = convert_positional(spec, &joined)?;
            values.insert(spec.name.clone(), value);
            position_index = raw_positionals.len();
            spec_index += 1;
            break;
        }

        let value = convert_positional(spec, &raw_positionals[position_index])?;
        values.insert(spec.name.clone(), value);
        position_index += 1;
        spec_index += 1;
    }

    while spec_index < specs.len() {
        let spec = &specs[spec_index];
        if spec.required && spec.default.is_none() {
            return Err(CommandParseError::MissingArgument {
                name: spec.name.clone(),
            });
        }
        spec_index += 1;
    }

    if position_index < raw_positionals.len() {
        return Err(CommandParseError::TooManyArguments {
            rest: raw_positionals[position_index..].join(" "),
        });
    }

    Ok(())
}

fn convert_positional(spec: &PositionalSpec, raw: &str) -> Result<ArgValue, CommandParseError> {
    let value = convert_value(&spec.arg_type, raw, &spec.name)?;
    if let Some(validator) = &spec.validator {
        validator
            .validate(&value)
            .map_err(|message| CommandParseError::Validation {
                name: spec.name.clone(),
                message,
            })?;
    }
    Ok(value)
}

fn apply_defaults(
    values: &mut HashMap<String, ArgValue>,
    positionals: &[PositionalSpec],
    options: &[OptionSpec],
) -> Result<(), CommandParseError> {
    for spec in positionals {
        if !values.contains_key(&spec.name) {
            if let Some(default) = &spec.default {
                values.insert(spec.name.clone(), default.clone());
            } else if spec.required {
                return Err(CommandParseError::MissingArgument {
                    name: spec.name.clone(),
                });
            }
        }
    }

    for option in options {
        if !values.contains_key(&option.canonical_name) {
            if let Some(default) = &option.default {
                values.insert(option.canonical_name.clone(), default.clone());
            } else if option.required {
                return Err(CommandParseError::MissingArgument {
                    name: option.canonical_name.clone(),
                });
            }
        }
    }

    Ok(())
}

fn store_option(values: &mut HashMap<String, ArgValue>, option: &OptionSpec, value: ArgValue) {
    if option.repeatable {
        append_list(values, &option.canonical_name, vec![value]);
    } else {
        values.insert(option.canonical_name.clone(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;

    fn pref_get_spec() -> ArgsSpec {
        ArgsSpec {
            positionals: vec![
                PositionalSpec::new("scope", ArgType::String).optional(),
                PositionalSpec::new("state", ArgType::String),
            ],
            options: Vec::new(),
        }
    }

    #[test]
    fn optional_scope_is_skipped_when_only_state_is_present() {
        let tokens = lexer::split("lang").unwrap();
        let parsed = parse_args(&tokens, &pref_get_spec()).unwrap();

        assert!(parsed.get_opt::<String>("scope").unwrap().is_none());
        assert_eq!(parsed.get::<String>("state").unwrap(), "lang");
    }

    #[test]
    fn optional_scope_is_consumed_when_both_values_are_present() {
        let tokens = lexer::split("user lang").unwrap();
        let parsed = parse_args(&tokens, &pref_get_spec()).unwrap();

        assert_eq!(
            parsed.get_opt::<String>("scope").unwrap().as_deref(),
            Some("user")
        );
        assert_eq!(parsed.get::<String>("state").unwrap(), "lang");
    }
}
