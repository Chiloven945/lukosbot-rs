use serde::Deserialize;
use serde::Serialize;
use serde::de::{self, Deserializer};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaveEntry {
    pub no: u32,
    pub uuid: String,
    pub text: Option<String>,
    pub image: Option<CaveImageBlob>,
    pub created_at: i64,
    pub created_by_platform: String,
    #[serde(default, deserialize_with = "deserialize_optional_user_id")]
    pub created_by_user_id: Option<String>,
    pub created_by_chat: Option<String>,
    pub source_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaveImageBlob {
    pub name: Option<String>,
    pub mime: Option<String>,
    pub base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaveMeta {
    #[serde(default = "default_next_no")]
    pub next_no: u32,
    #[serde(default)]
    pub active_numbers: Vec<u32>,
}

impl Default for CaveMeta {
    fn default() -> Self {
        Self {
            next_no: default_next_no(),
            active_numbers: Vec::new(),
        }
    }
}

fn default_next_no() -> u32 {
    1
}

fn deserialize_optional_user_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value)),
        Some(serde_json::Value::Number(value)) => Ok(Some(value.to_string())),
        Some(_) => Err(de::Error::custom(
            "createdByUserId must be a string, number, or null",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_legacy_numeric_user_ids() {
        let value = json!({
            "no": 1,
            "uuid": "u",
            "text": "hello",
            "image": null,
            "createdAt": 1,
            "createdByPlatform": "Telegram",
            "createdByUserId": 42,
            "createdByChat": "chat",
            "sourceMessageId": "m"
        });
        let entry: CaveEntry = serde_json::from_value(value).unwrap();
        assert_eq!(entry.created_by_user_id.as_deref(), Some("42"));
    }

    #[test]
    fn accepts_string_user_ids_and_meta_defaults() {
        let entry: CaveEntry = serde_json::from_value(json!({
            "no": 1,
            "uuid": "u",
            "text": null,
            "image": null,
            "createdAt": 1,
            "createdByPlatform": "Discord",
            "createdByUserId": "42",
            "createdByChat": null,
            "sourceMessageId": null
        }))
        .unwrap();
        assert_eq!(entry.created_by_user_id.as_deref(), Some("42"));

        let meta: CaveMeta = serde_json::from_value(json!({})).unwrap();
        assert_eq!(meta.next_no, 1);
        assert!(meta.active_numbers.is_empty());
    }
}
