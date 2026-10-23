use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlatformsSection {
    pub telegram: TelegramSection,
    pub discord: DiscordSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelegramSection {
    pub enabled: bool,
    #[serde(deserialize_with = "deserialize_optional_secret")]
    pub token: Option<SecretString>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiscordSection {
    pub enabled: bool,
    #[serde(deserialize_with = "deserialize_optional_secret")]
    pub token: Option<SecretString>,
}

impl TelegramSection {
    pub fn token(&self) -> Option<&str> {
        self.token.as_ref().map(|token| token.expose_secret())
    }
}

impl DiscordSection {
    pub fn token(&self) -> Option<&str> {
        self.token.as_ref().map(|token| token.expose_secret())
    }
}

/// Nesting separators and prefixes are not applied to secrets by the generic config layer, so
/// this visitor accepts the scalar types that `try_parsing` may infer from an environment
/// variable. This keeps numeric-looking tokens working.
fn deserialize_optional_secret<'de, D>(deserializer: D) -> Result<Option<SecretString>, D::Error>
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
            Ok(Some(SecretString::from(value.to_string())))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(SecretString::from(value.to_string())))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(SecretString::from(value.to_owned())))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(SecretString::from(value)))
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
            Ok(SecretString::from(value.to_string()))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(SecretString::from(value.to_string()))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(SecretString::from(value.to_owned()))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(SecretString::from(value))
        }
    }

    deserializer.deserialize_option(OptionVisitor)
}
