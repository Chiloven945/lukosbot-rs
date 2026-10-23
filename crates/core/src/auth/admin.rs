use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::sync::Mutex;
use tracing::warn;

use lukosbot_domain::{PlatformKind, PlatformUserId};

use crate::ports::{StateKey, StateStore, StateStoreError};

use super::error::AdminError;

const NS_AUTH: &str = "auth";
const KEY_BOT_ADMINS: &str = "bot_admins";
const PLATFORMS: [PlatformKind; 2] = [PlatformKind::Telegram, PlatformKind::Discord];

/// A platform-qualified administrator identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AdminIdentity {
    pub platform: PlatformKind,
    pub user_id: PlatformUserId,
}

impl AdminIdentity {
    pub fn new(platform: PlatformKind, user_id: PlatformUserId) -> Self {
        Self { platform, user_id }
    }
}

/// Result of an idempotent dynamic-admin mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminMutation {
    Added,
    AlreadyPresent,
    Removed,
    NotPresent,
}

/// Bot administrators from bootstrap configuration plus the dynamic global state entry.
pub struct BotAdminService {
    store: Arc<dyn StateStore>,
    bootstrap_admins: BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>,
    mutation_lock: Mutex<()>,
}

impl BotAdminService {
    pub fn new(
        store: Arc<dyn StateStore>,
        bootstrap_admins: BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>,
    ) -> Self {
        Self {
            store,
            bootstrap_admins,
            mutation_lock: Mutex::new(()),
        }
    }

    pub fn bootstrap_admins(&self) -> &BTreeMap<PlatformKind, BTreeSet<PlatformUserId>> {
        &self.bootstrap_admins
    }

    pub async fn is_bot_admin(&self, identity: &AdminIdentity) -> Result<bool, AdminError> {
        if self
            .bootstrap_admins
            .get(&identity.platform)
            .is_some_and(|admins| admins.contains(&identity.user_id))
        {
            return Ok(true);
        }

        let dynamic = self.load_dynamic().await?;
        Ok(dynamic
            .get(&identity.platform)
            .is_some_and(|admins| admins.contains(&identity.user_id)))
    }

    pub async fn list_effective_admins(
        &self,
    ) -> Result<BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>, AdminError> {
        let dynamic = self.load_dynamic().await?;
        Ok(merge_effective(&self.bootstrap_admins, &dynamic))
    }

    pub async fn add_dynamic_admin(
        &self,
        identity: AdminIdentity,
    ) -> Result<AdminMutation, AdminError> {
        let _guard = self.mutation_lock.lock().await;
        let mut dynamic = self.load_dynamic().await?;

        let admins = dynamic.entry(identity.platform).or_default();
        if !admins.insert(identity.user_id) {
            return Ok(AdminMutation::AlreadyPresent);
        }

        self.save_dynamic(&dynamic).await?;
        Ok(AdminMutation::Added)
    }

    pub async fn remove_dynamic_admin(
        &self,
        identity: AdminIdentity,
    ) -> Result<AdminMutation, AdminError> {
        let _guard = self.mutation_lock.lock().await;
        let mut dynamic = self.load_dynamic().await?;

        let Some(admins) = dynamic.get_mut(&identity.platform) else {
            return Ok(AdminMutation::NotPresent);
        };
        if !admins.remove(&identity.user_id) {
            return Ok(AdminMutation::NotPresent);
        }

        let effective = merge_effective(&self.bootstrap_admins, &dynamic);
        if effective.values().all(BTreeSet::is_empty) {
            return Err(AdminError::LastAdminProtection);
        }

        self.save_dynamic(&dynamic).await?;
        Ok(AdminMutation::Removed)
    }

    async fn load_dynamic(
        &self,
    ) -> Result<BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>, AdminError> {
        let key = StateKey::global(NS_AUTH, KEY_BOT_ADMINS)?;
        let value = match self.store.get(&key).await {
            Ok(value) => value,
            Err(StateStoreError::Codec(_)) => return Err(AdminError::CorruptAdminList),
            Err(error) => return Err(AdminError::Storage(error)),
        };

        let Some(value) = value else {
            return Ok(BTreeMap::new());
        };

        parse_dynamic_admins(&value.value)
    }

    async fn save_dynamic(
        &self,
        admins: &BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>,
    ) -> Result<(), AdminError> {
        let key = StateKey::global(NS_AUTH, KEY_BOT_ADMINS)?;
        let mut root = Map::new();

        for platform in PLATFORMS {
            let ids = admins
                .get(&platform)
                .into_iter()
                .flatten()
                .map(|id| Value::String(id.as_str().to_owned()))
                .collect();
            root.insert(platform.as_str().to_owned(), Value::Array(ids));
        }

        self.store.put(key, Value::Object(root), None).await?;
        Ok(())
    }
}

fn merge_effective(
    bootstrap: &BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>,
    dynamic: &BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>,
) -> BTreeMap<PlatformKind, BTreeSet<PlatformUserId>> {
    let mut effective = BTreeMap::new();
    for platform in PLATFORMS {
        effective.insert(platform, BTreeSet::new());
    }

    for source in [bootstrap, dynamic] {
        for (platform, ids) in source {
            effective
                .entry(*platform)
                .or_default()
                .extend(ids.iter().cloned());
        }
    }

    effective
}

fn parse_dynamic_admins(
    value: &Value,
) -> Result<BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>, AdminError> {
    let Some(root) = value.as_object() else {
        return Err(AdminError::CorruptAdminList);
    };

    let mut admins = BTreeMap::new();
    for (platform_name, raw_ids) in root {
        let platform =
            match PlatformKind::from_str(platform_name.trim().to_ascii_lowercase().as_str()) {
                Ok(platform) => platform,
                Err(_) => {
                    warn!(platform = %platform_name, "ignoring unknown bot-admin platform key");
                    continue;
                }
            };

        let Some(raw_ids) = raw_ids.as_array() else {
            return Err(AdminError::CorruptAdminList);
        };

        let entry = admins.entry(platform).or_insert_with(BTreeSet::new);
        for raw_id in raw_ids {
            let Some(id) = parse_admin_id(raw_id) else {
                warn!(platform = %platform, "ignoring invalid bot-admin id entry");
                continue;
            };
            match PlatformUserId::new(id.to_string()) {
                Ok(id) => {
                    entry.insert(id);
                }
                Err(error) => {
                    warn!(platform = %platform, %error, "ignoring invalid bot-admin id");
                }
            }
        }
    }

    Ok(admins)
}

fn parse_admin_id(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok())),
        Value::String(value) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}
