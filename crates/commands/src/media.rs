pub mod cave;
pub mod e621;

use std::sync::Arc;

use cave::CaveService;
use lukosbot_integrations::E621Service;

/// Concrete media services injected by the composition root.
#[derive(Clone, Default)]
pub struct MediaCommandServices {
    pub cave: Option<Arc<CaveService>>,
    pub e621: Option<Arc<dyn E621Service>>,
    pub e621_limit: u32,
}
