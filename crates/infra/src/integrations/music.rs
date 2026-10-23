use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_integrations::{IntegrationError, MusicLink, MusicPlatform, MusicService, TrackInfo};

use crate::http::HttpClient;

pub mod soundcloud;
pub mod spotify;

pub use soundcloud::SoundCloudClient;
pub use spotify::SpotifyClient;

/// The aggregate music service. Providers are only constructed when configuration makes them
/// usable; their credentials are never fetched until the first command call.
pub struct MusicClient {
    spotify: Option<SpotifyClient>,
    soundcloud: Option<SoundCloudClient>,
}

impl MusicClient {
    pub fn new(spotify: Option<SpotifyClient>, soundcloud: Option<SoundCloudClient>) -> Self {
        Self {
            spotify,
            soundcloud,
        }
    }

    pub fn with_http(
        http: Arc<HttpClient>,
        spotify: Option<(secrecy::SecretString, secrecy::SecretString)>,
        soundcloud: Option<secrecy::SecretString>,
    ) -> Self {
        let spotify = spotify.map(|(client_id, client_secret)| {
            SpotifyClient::new(Arc::clone(&http), client_id, client_secret)
        });
        let soundcloud =
            soundcloud.map(|client_id| SoundCloudClient::new(Arc::clone(&http), client_id));
        Self::new(spotify, soundcloud)
    }
}

#[async_trait]
impl MusicService for MusicClient {
    fn available_platforms(&self) -> Vec<MusicPlatform> {
        let mut platforms = Vec::new();
        if self.spotify.is_some() {
            platforms.push(MusicPlatform::Spotify);
        }
        if self.soundcloud.is_some() {
            platforms.push(MusicPlatform::SoundCloud);
        }
        platforms
    }

    async fn search(
        &self,
        platform: MusicPlatform,
        query: &str,
    ) -> Result<TrackInfo, IntegrationError> {
        match platform {
            MusicPlatform::Spotify => match &self.spotify {
                Some(client) => client.search(query).await,
                None => Err(IntegrationError::NotConfigured {
                    provider: "spotify".to_owned(),
                }),
            },
            MusicPlatform::SoundCloud => match &self.soundcloud {
                Some(client) => client.search(query).await,
                None => Err(IntegrationError::NotConfigured {
                    provider: "soundcloud".to_owned(),
                }),
            },
        }
    }

    async fn resolve_link(&self, link: &MusicLink) -> Result<TrackInfo, IntegrationError> {
        match link.platform {
            MusicPlatform::Spotify => match &self.spotify {
                Some(client) => client.resolve_link(link).await,
                None => Err(IntegrationError::NotConfigured {
                    provider: "spotify".to_owned(),
                }),
            },
            MusicPlatform::SoundCloud => match &self.soundcloud {
                Some(client) => client.resolve_link(link).await,
                None => Err(IntegrationError::NotConfigured {
                    provider: "soundcloud".to_owned(),
                }),
            },
        }
    }
}
