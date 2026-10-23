use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use serde::Deserialize;

use lukosbot_domain::{PlatformKind, PlatformUserId};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SecuritySection {
    pub bootstrap_bot_admins: BTreeMap<String, Vec<i64>>,
}

impl SecuritySection {
    /// Compiles the raw platform map into validated administrator identities.
    pub fn compile(&self) -> Result<BTreeMap<PlatformKind, BTreeSet<PlatformUserId>>, Vec<String>> {
        let mut issues = Vec::new();
        let mut admins: BTreeMap<PlatformKind, BTreeSet<PlatformUserId>> = BTreeMap::new();

        for (raw_platform, raw_ids) in &self.bootstrap_bot_admins {
            let platform_name = raw_platform.trim().to_ascii_lowercase();
            let platform = match PlatformKind::from_str(&platform_name) {
                Ok(platform) => platform,
                Err(_) => {
                    issues.push(format!(
                        "security.bootstrap_bot_admins contains unknown platform `{raw_platform}`"
                    ));
                    continue;
                }
            };

            let entry = admins.entry(platform).or_default();
            for id in raw_ids {
                match PlatformUserId::new(id.to_string()) {
                    Ok(id) => {
                        entry.insert(id);
                    }
                    Err(error) => issues.push(format!(
                        "security.bootstrap_bot_admins.{platform_name} contains invalid id {id}: {error}"
                    )),
                }
            }
        }

        if issues.is_empty() {
            Ok(admins)
        } else {
            Err(issues)
        }
    }
}
