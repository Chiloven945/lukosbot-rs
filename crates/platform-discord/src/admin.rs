use std::collections::HashMap;
use std::time::{Duration, Instant};

use serenity::all::Permissions;
use tokio::sync::Mutex;

use lukosbot_domain::PlatformKind;

const DEFAULT_TTL: Duration = Duration::from_secs(60);
const MAX_ENTRIES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PermissionCacheKey {
    platform: PlatformKind,
    channel_id: u64,
    user_id: u64,
}

impl PermissionCacheKey {
    pub(crate) fn new(platform: PlatformKind, channel_id: u64, user_id: u64) -> Self {
        Self {
            platform,
            channel_id,
            user_id,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CacheEntry {
    value: bool,
    expires_at: Instant,
}

/// Bounded cache of chat-admin decisions captured from gateway message events.
#[derive(Debug)]
pub(crate) struct DiscordPermissionCache {
    entries: Mutex<HashMap<PermissionCacheKey, CacheEntry>>,
    ttl: Duration,
}

impl Default for DiscordPermissionCache {
    fn default() -> Self {
        Self::new(DEFAULT_TTL)
    }
}

impl DiscordPermissionCache {
    pub(crate) fn new(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    pub(crate) async fn get(&self, key: &PermissionCacheKey) -> Option<bool> {
        let now = Instant::now();
        let mut entries = self.entries.lock().await;
        match entries.get(key) {
            Some(entry) if entry.expires_at > now => Some(entry.value),
            Some(_) => {
                entries.remove(key);
                None
            }
            None => None,
        }
    }

    pub(crate) async fn put(&self, key: PermissionCacheKey, value: bool) {
        let now = Instant::now();
        let mut entries = self.entries.lock().await;
        entries.retain(|_, entry| entry.expires_at > now);

        if entries.len() >= MAX_ENTRIES {
            entries.clear();
        }

        entries.insert(
            key,
            CacheEntry {
                value,
                expires_at: now + self.ttl,
            },
        );
    }
}

/// Maps resolved guild permissions to the legacy bot's chat-admin rule.
pub(crate) fn is_chat_admin_permissions(permissions: Permissions) -> bool {
    permissions.contains(Permissions::ADMINISTRATOR)
        || permissions.contains(Permissions::MANAGE_GUILD)
        || permissions.contains(Permissions::MANAGE_CHANNELS)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serenity::all::UserId;

    use super::*;

    fn key(user_id: u64) -> PermissionCacheKey {
        PermissionCacheKey::new(PlatformKind::Discord, 20, user_id)
    }

    #[test]
    fn guild_and_channel_permissions_map_to_chat_admin() {
        assert!(is_chat_admin_permissions(Permissions::ADMINISTRATOR));
        assert!(is_chat_admin_permissions(Permissions::MANAGE_GUILD));
        assert!(is_chat_admin_permissions(Permissions::MANAGE_CHANNELS));
        assert!(!is_chat_admin_permissions(Permissions::SEND_MESSAGES));
    }

    #[tokio::test]
    async fn cache_key_includes_channel_and_user() {
        let cache = Arc::new(DiscordPermissionCache::new(Duration::from_secs(60)));
        cache.put(key(30), true).await;

        assert_eq!(cache.get(&key(30)).await, Some(true));
        assert_eq!(
            cache
                .get(&PermissionCacheKey::new(PlatformKind::Discord, 21, 30))
                .await,
            None
        );
        assert_eq!(
            cache
                .get(&PermissionCacheKey::new(PlatformKind::Discord, 20, 31))
                .await,
            None
        );
    }

    #[test]
    fn user_id_is_part_of_the_cache_identity() {
        let key = PermissionCacheKey::new(PlatformKind::Discord, 20, 30);
        assert_eq!(key.user_id, UserId::new(30).get());
    }
}
