use std::collections::HashMap;
use std::time::{Duration, Instant};

use teloxide::types::{ChatMemberKind, UserId};
use tokio::sync::Mutex;

use lukosbot_domain::PlatformKind;

const DEFAULT_TTL: Duration = Duration::from_secs(60);
const MAX_ENTRIES: usize = 1024;

/// Cache key deliberately includes the platform, even though this cache belongs to one adapter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AdminCacheKey {
    platform: PlatformKind,
    chat_id: i64,
    user_id: UserId,
}

impl AdminCacheKey {
    pub(crate) fn new(platform: PlatformKind, chat_id: i64, user_id: UserId) -> Self {
        Self {
            platform,
            chat_id,
            user_id,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CacheEntry {
    value: bool,
    expires_at: Instant,
}

#[derive(Debug)]
pub(crate) struct TelegramAdminCache {
    entries: Mutex<HashMap<AdminCacheKey, CacheEntry>>,
    ttl: Duration,
}

impl Default for TelegramAdminCache {
    fn default() -> Self {
        Self::new(DEFAULT_TTL)
    }
}

impl TelegramAdminCache {
    pub(crate) fn new(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    pub(crate) async fn get(&self, key: &AdminCacheKey) -> Option<bool> {
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

    pub(crate) async fn put(&self, key: AdminCacheKey, value: bool) {
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

pub(crate) fn is_admin_member(kind: &ChatMemberKind) -> bool {
    matches!(
        kind,
        ChatMemberKind::Owner(_) | ChatMemberKind::Administrator(_)
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn key(user_id: u64) -> AdminCacheKey {
        AdminCacheKey::new(PlatformKind::Telegram, -100, UserId(user_id))
    }

    #[test]
    fn owner_and_administrator_are_admins() {
        let admin: teloxide::types::ChatMember = serde_json::from_value(serde_json::json!({
            "user": {
                "id": 7,
                "is_bot": false,
                "first_name": "A"
            },
            "status": "administrator",
            "can_be_edited": false,
            "is_anonymous": false,
            "can_manage_chat": true,
            "can_delete_messages": true,
            "can_manage_video_chats": true,
            "can_restrict_members": true,
            "can_promote_members": false,
            "can_change_info": false,
            "can_invite_users": true
        }))
        .expect("valid administrator fixture");

        assert!(is_admin_member(&admin.kind));
    }

    #[test]
    fn creator_status_is_owner_and_admins() {
        let owner: teloxide::types::ChatMember = serde_json::from_value(serde_json::json!({
            "user": {
                "id": 7,
                "is_bot": false,
                "first_name": "A"
            },
            "status": "creator",
            "is_anonymous": false
        }))
        .expect("valid owner fixture");

        assert!(is_admin_member(&owner.kind));
    }

    #[test]
    fn regular_member_is_not_admin() {
        let member: teloxide::types::ChatMember = serde_json::from_value(serde_json::json!({
            "user": {
                "id": 7,
                "is_bot": false,
                "first_name": "A"
            },
            "status": "member"
        }))
        .expect("valid member fixture");

        assert!(!is_admin_member(&member.kind));
    }

    #[tokio::test]
    async fn cache_key_includes_platform_chat_and_user() {
        let cache = Arc::new(TelegramAdminCache::new(Duration::from_secs(60)));
        cache.put(key(7), true).await;

        assert_eq!(cache.get(&key(7)).await, Some(true));
        assert_eq!(
            cache
                .get(&AdminCacheKey::new(PlatformKind::Discord, -100, UserId(7)))
                .await,
            None
        );
        assert_eq!(
            cache
                .get(&AdminCacheKey::new(PlatformKind::Telegram, -101, UserId(7)))
                .await,
            None
        );
    }
}
