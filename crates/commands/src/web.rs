pub mod wiki;

use std::sync::Arc;

use lukosbot_integrations::WikiService;

/// Concrete web services injected by the composition root.
#[derive(Clone, Default)]
pub struct WebCommandServices {
    pub wiki: Option<Arc<dyn WikiService>>,
}
