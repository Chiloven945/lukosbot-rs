use std::fmt;

use secrecy::SecretString;
use serde::de::{self, Deserializer, Visitor};

/// Deserializes a secret from TOML or from an environment variable.
///
/// The generic config layer may infer numeric-looking environment values as integers, so this
/// visitor accepts strings and integer scalars. It never echoes the value in a parse error.
pub(crate) fn deserialize_optional_secret<'de, D>(
    deserializer: D,
) -> Result<Option<SecretString>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionVisitor;

    impl<'de> Visitor<'de> for OptionVisitor {
        type Value = Option<SecretString>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a secret string, an integer, or null")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(secret_from(value.to_string())))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(secret_from(value.to_string())))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(secret_from(value.to_owned())))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(secret_from(value)))
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(ScalarVisitor).map(Some)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    struct ScalarVisitor;

    impl Visitor<'_> for ScalarVisitor {
        type Value = SecretString;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a secret string or an integer")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(secret_from(value.to_string()))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(secret_from(value.to_string()))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(secret_from(value.to_owned()))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(secret_from(value))
        }
    }

    deserializer.deserialize_option(OptionVisitor)
}

fn secret_from(value: String) -> SecretString {
    SecretString::from(value)
}
