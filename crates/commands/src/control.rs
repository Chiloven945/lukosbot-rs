use std::sync::Arc;

use lukosbot_core::auth::{AuthorizationService, BotAdminService};
use lukosbot_core::state::StateService;

/// Optional control-command dependencies.
///
/// A command is registered only when its flag is enabled and all required services are present.
#[derive(Clone, Default)]
pub struct ControlCommands {
    pub admin: bool,
    pub pref: bool,
    pub bot_admins: Option<Arc<BotAdminService>>,
    pub authorization: Option<Arc<AuthorizationService>>,
    pub state_service: Option<Arc<StateService>>,
}

impl ControlCommands {
    pub fn all_disabled() -> Self {
        Self::default()
    }
}
