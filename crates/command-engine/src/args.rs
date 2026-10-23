use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use thiserror::Error;

/// The type declared for a positional argument or option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgType {
    String,
    I32,
    I64,
    Bool,
    Choice(Vec<String>),
}

impl ArgType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::Bool => "bool",
            Self::Choice(_) => "choice",
        }
    }

    pub fn accepts(&self, value: &ArgValue) -> bool {
        match (self, value) {
            (Self::String, ArgValue::String(_)) => true,
            (Self::I32, ArgValue::I32(_)) => true,
            (Self::I64, ArgValue::I64(_)) => true,
            (Self::Bool, ArgValue::Bool(_)) => true,
            (Self::Choice(choices), ArgValue::String(value)) => choices.contains(value),
            _ => false,
        }
    }
}

/// A parsed argument value.
///
/// The engine intentionally uses a small closed value type instead of `Any` and runtime
/// downcasts. Command authors retrieve values through [`ParsedArgs::get`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgValue {
    String(String),
    I32(i32),
    I64(i64),
    Bool(bool),
    List(Vec<ArgValue>),
}

impl ArgValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::String(_) => "string",
            Self::I32(_) => "i32",
            Self::I64(_) => "i64",
            Self::Bool(_) => "bool",
            Self::List(_) => "list",
        }
    }
}

impl fmt::Display for ArgValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => formatter.write_str(value),
            Self::I32(value) => write!(formatter, "{value}"),
            Self::I64(value) => write!(formatter, "{value}"),
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::List(values) => {
                formatter.write_str("[")?;
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{value}")?;
                }
                formatter.write_str("]")
            }
        }
    }
}

type ValidatorFn = dyn Fn(&ArgValue) -> Result<(), String> + Send + Sync;

/// A named argument validator. Validators only decide whether a value is acceptable; they never
/// send messages themselves.
#[derive(Clone)]
pub struct Validator(Arc<ValidatorFn>);

impl fmt::Debug for Validator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Validator(..)")
    }
}

impl Validator {
    pub fn new(
        validator: impl Fn(&ArgValue) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(validator))
    }

    pub fn validate(&self, value: &ArgValue) -> Result<(), String> {
        self.0(value)
    }
}

/// Converts an [`ArgValue`] into a concrete Rust type.
pub trait FromArgValue: Sized {
    fn from_arg_value(value: &ArgValue) -> Option<Self>;
}

impl FromArgValue for String {
    fn from_arg_value(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::String(value) => Some(value.clone()),
            _ => None,
        }
    }
}

impl FromArgValue for i32 {
    fn from_arg_value(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::I32(value) => Some(*value),
            _ => None,
        }
    }
}

impl FromArgValue for i64 {
    fn from_arg_value(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::I64(value) => Some(*value),
            _ => None,
        }
    }
}

impl FromArgValue for bool {
    fn from_arg_value(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::Bool(value) => Some(*value),
            _ => None,
        }
    }
}

impl<T: FromArgValue> FromArgValue for Vec<T> {
    fn from_arg_value(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::List(values) => values
                .iter()
                .map(T::from_arg_value)
                .collect::<Option<Vec<_>>>(),
            _ => None,
        }
    }
}

/// Typed values produced by the argv parser.
#[derive(Debug, Clone, Default)]
pub struct ParsedArgs {
    values: HashMap<String, ArgValue>,
    raw_positionals: Vec<String>,
}

impl ParsedArgs {
    pub(crate) fn new(values: HashMap<String, ArgValue>, raw_positionals: Vec<String>) -> Self {
        Self {
            values,
            raw_positionals,
        }
    }

    pub fn get<T: FromArgValue>(&self, name: &str) -> Result<T, ArgAccessError> {
        let value = self
            .values
            .get(name)
            .ok_or_else(|| ArgAccessError::Missing {
                name: name.to_owned(),
            })?;

        T::from_arg_value(value).ok_or_else(|| ArgAccessError::TypeMismatch {
            name: name.to_owned(),
            expected: std::any::type_name::<T>(),
            actual: value.type_name(),
        })
    }

    pub fn get_opt<T: FromArgValue>(&self, name: &str) -> Result<Option<T>, ArgAccessError> {
        match self.values.get(name) {
            Some(value) => {
                T::from_arg_value(value)
                    .map(Some)
                    .ok_or_else(|| ArgAccessError::TypeMismatch {
                        name: name.to_owned(),
                        expected: std::any::type_name::<T>(),
                        actual: value.type_name(),
                    })
            }
            None => Ok(None),
        }
    }

    pub fn raw_positionals(&self) -> &[String] {
        &self.raw_positionals
    }

    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }
}

/// A typed argument lookup failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ArgAccessError {
    #[error("缺少参数：{name}")]
    Missing { name: String },

    #[error("参数 {name} 类型不匹配：期望 {expected}，实际 {actual}")]
    TypeMismatch {
        name: String,
        expected: &'static str,
        actual: &'static str,
    },
}
