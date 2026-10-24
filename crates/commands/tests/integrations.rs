use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio_util::sync::CancellationToken;
use url::Url;

use lukosbot_command_engine::CommandMessageHandler;
use lukosbot_commands::{
    BasicCommandsConfig, ControlCommands, IntegrationCommandServices, MediaCommandServices,
    MediaCommandsConfig, RegistryBuildDependencies, RuntimeInfo, ThirdPartyCommandsConfig,
    WebCommandServices, WebCommandsConfig, build_registry_with_dependencies,
};
use lukosbot_core::policy::{AllowAllPolicy, PolicyEngine, PolicyMatch, PolicyRule};
use lukosbot_core::testing::{MemoryStateStore, RecordingAdapter, SentRecord};
use lukosbot_core::{AppRuntime, RuntimeSettings, StateStore};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, OutboundPart, PlatformChatId, PlatformKind,
    PlatformUserId, Sender,
};
use lukosbot_integrations::{
    BilibiliOwner, BilibiliService, BilibiliStats, BilibiliTarget, BilibiliVideo, GitHubRepository,
    GitHubRepositoryBrief, GitHubSearchParams, GitHubSearchResult, GitHubService, GitHubUser,
    IntegrationError, IpData, IpLookupService, IpProviderId, IpQueryResult, MusicLink,
    MusicPlatform, MusicService, TrackInfo, VideoId, WhoisRecord, WhoisService,
};

struct FakeGitHub;

#[async_trait]
impl GitHubService for FakeGitHub {
    async fn user(&self, username: &str) -> Result<GitHubUser, IntegrationError> {
        Ok(GitHubUser {
            login: username.to_owned(),
            name: Some("Test User".to_owned()),
            html_url: Some(Url::parse("https://github.com/example")?),
            public_repos: 1,
            followers: 2,
            following: 3,
        })
    }

    async fn repository(
        &self,
        owner: &str,
        repository: &str,
    ) -> Result<GitHubRepository, IntegrationError> {
        Ok(GitHubRepository {
            full_name: format!("{owner}/{repository}"),
            html_url: Some(Url::parse("https://github.com/example/repo")?),
            language: Some("Rust".to_owned()),
            stargazers_count: 4,
            forks_count: 5,
            description: Some("Fake repository".to_owned()),
        })
    }

    async fn search(
        &self,
        params: &GitHubSearchParams,
    ) -> Result<GitHubSearchResult, IntegrationError> {
        assert_eq!(params.top, 2);
        assert_eq!(params.language.as_deref(), Some("rust"));
        Ok(GitHubSearchResult {
            total_count: 1,
            items: vec![GitHubRepositoryBrief {
                full_name: "owner/repo".to_owned(),
                html_url: Some(Url::parse("https://github.com/owner/repo")?),
                stargazers_count: 6,
            }],
        })
    }
}

struct FakeBilibili;

#[async_trait]
impl BilibiliService for FakeBilibili {
    async fn query(&self, target: &BilibiliTarget) -> Result<BilibiliVideo, IntegrationError> {
        assert!(matches!(target, BilibiliTarget::Video(_)));
        Ok(BilibiliVideo {
            id: VideoId::Bv("BV1GJ411x7h7".to_owned()),
            url: Url::parse("https://www.bilibili.com/video/BV1GJ411x7h7")?,
            title: "Fake video".to_owned(),
            category: Some("科技".to_owned()),
            description: Some("Fake description".to_owned()),
            cover_url: Some(Url::parse("https://example.com/cover.jpg")?),
            published_at: Some(Utc::now()),
            owner: BilibiliOwner {
                mid: Some(42),
                name: Some("Uploader".to_owned()),
                followers: Some(1234),
            },
            stats: BilibiliStats {
                views: 1000,
                danmaku: 20,
                replies: 30,
                likes: 40,
                coins: 50,
                favorites: 60,
                shares: 70,
            },
            pages: 2,
        })
    }
}

struct FakeIp;

#[async_trait]
impl IpLookupService for FakeIp {
    async fn query(
        &self,
        ip: IpAddr,
        providers: &[IpProviderId],
    ) -> Result<IpQueryResult, IntegrationError> {
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)));
        assert!(!providers.is_empty());
        Ok(IpQueryResult {
            provider: providers[0],
            data: IpData {
                ip,
                country: Some("Australia".to_owned()),
                country_code: Some("AU".to_owned()),
                region: None,
                region_code: None,
                city: Some("Brisbane".to_owned()),
                postal_code: None,
                latitude: None,
                longitude: None,
                timezone: None,
                asn: Some("AS13335".to_owned()),
                organization: Some("Example".to_owned()),
                isp: Some("Example ISP".to_owned()),
                risk: None,
            },
            failed_providers: Vec::new(),
        })
    }
}

struct FakeMusic;

#[async_trait]
impl MusicService for FakeMusic {
    fn available_platforms(&self) -> Vec<MusicPlatform> {
        vec![MusicPlatform::Spotify, MusicPlatform::SoundCloud]
    }

    async fn search(
        &self,
        platform: MusicPlatform,
        query: &str,
    ) -> Result<TrackInfo, IntegrationError> {
        assert_eq!(query, "track");
        Ok(track(platform))
    }

    async fn resolve_link(&self, link: &MusicLink) -> Result<TrackInfo, IntegrationError> {
        assert_eq!(link.platform, MusicPlatform::Spotify);
        Ok(track(link.platform))
    }
}

fn track(platform: MusicPlatform) -> TrackInfo {
    TrackInfo {
        platform,
        id: "id".to_owned(),
        title: "Track".to_owned(),
        artist: "Artist".to_owned(),
        album: Some("Album".to_owned()),
        cover_url: None,
        track_url: Some(Url::parse("https://open.spotify.com/track/id").unwrap()),
        duration: Some(Duration::from_secs(213)),
    }
}

struct FakeWhois;

#[async_trait]
impl WhoisService for FakeWhois {
    async fn query(&self, domain: &str) -> Result<WhoisRecord, IntegrationError> {
        assert_eq!(domain, "example.com");
        Ok(WhoisRecord {
            domain: domain.to_owned(),
            server: "whois.example.test".to_owned(),
            text: "Domain: example.com\nRegistrar: Example".to_owned(),
            truncated: false,
        })
    }
}

fn address() -> ChatAddress {
    ChatAddress::new(
        PlatformKind::Telegram,
        PlatformChatId::new("100").unwrap(),
        ChatKind::Private,
    )
}

fn inbound(text: &str) -> InboundMessage {
    InboundMessage {
        address: address(),
        sender: Sender {
            id: PlatformUserId::new("7").unwrap(),
            display_name: None,
            username: None,
        },
        message_id: None,
        received_at: Utc::now(),
        parts: vec![InboundPart::Text(text.to_owned())],
        reply_to: None,
        quoted: None,
        context: Default::default(),
    }
}

fn text_of(record: &SentRecord) -> String {
    match &record.message.parts[0] {
        OutboundPart::Text(text) => text.clone(),
        _ => panic!("expected text"),
    }
}

#[tokio::test]
async fn integration_commands_are_reachable_with_aliases_and_arguments() {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    for text in [
        "/github",
        "/gh user octocat",
        "/github repo owner/repo",
        "/github search rust --top=2 --lang=rust",
        "/bilibili BV1GJ411x7h7 -i",
        "/bili av1",
        "/ip 1.1.1.1",
        "/ip --provider=ipsb 1.1.1.1",
        "/music track",
        "/music s track",
        "/music link https://open.spotify.com/track/id",
        "/whois example.com",
        "/help",
    ] {
        adapter.push_inbound(inbound(text));
    }

    let services = IntegrationCommandServices {
        github: Some(Arc::new(FakeGitHub)),
        bilibili: Some(Arc::new(FakeBilibili)),
        ip: Some(Arc::new(FakeIp)),
        music: Some(Arc::new(FakeMusic)),
        whois: Some(Arc::new(FakeWhois)),
    };
    let registry = build_registry_with_dependencies(RegistryBuildDependencies {
        basic: BasicCommandsConfig {
            ping: false,
            echo: false,
            coin: false,
            dice: false,
            luck: false,
            twenty_four: false,
            weather: false,
        },
        media: MediaCommandsConfig::all_disabled(),
        web: WebCommandsConfig::all_disabled(),
        third_party: ThirdPartyCommandsConfig::default(),
        runtime_info: Arc::new(RuntimeInfo::new("test")),
        twenty_four: None,
        controls: ControlCommands::default(),
        services,
        media_services: MediaCommandServices::default(),
        web_services: WebCommandServices::default(),
    })
    .unwrap();
    let handler = Arc::new(
        CommandMessageHandler::new("/", Arc::new(registry), Arc::new(AllowAllPolicy)).unwrap(),
    );
    let runtime = AppRuntime::builder(
        RuntimeSettings {
            inbound_capacity: 32,
            outbound_capacity: 32,
            handler_timeout: Duration::from_secs(2),
            send_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(2),
        },
        handler,
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_platform(adapter.clone())
    .unwrap()
    .build();

    let cancel = CancellationToken::new();
    let shutdown = cancel.clone();
    let task =
        tokio::spawn(async move { runtime.run(async move { shutdown.cancelled().await }).await });

    adapter.wait_for_sends(13).await;
    let replies: Vec<String> = adapter.sent().iter().map(text_of).collect();
    assert!(replies[0].contains("用法："));
    assert!(replies[1].contains("Test User"));
    assert!(replies[2].contains("owner/repo"));
    assert!(replies[3].contains("owner/repo - 6★"));
    assert!(replies[4].contains("（2P）"));
    assert!(replies[5].contains("Fake video"));
    assert!(replies[6].contains("IP 地址：1.1.1.1"));
    assert!(replies[7].contains("数据源：ipsb"));
    assert!(replies[8].contains("平台：Spotify"));
    assert!(replies[9].starts_with("平台：Spotify"));
    assert!(replies[10].contains("链接：https://open.spotify.com/track/id"));
    assert!(replies[11].contains("Registrar: Example"));
    assert!(replies[12].contains("github"));

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");
}

struct PanicGitHub;

#[async_trait]
impl GitHubService for PanicGitHub {
    async fn user(&self, _username: &str) -> Result<GitHubUser, IntegrationError> {
        panic!("policy must reject before calling the service")
    }

    async fn repository(
        &self,
        _owner: &str,
        _repository: &str,
    ) -> Result<GitHubRepository, IntegrationError> {
        panic!("policy must reject before calling the service")
    }

    async fn search(
        &self,
        _params: &GitHubSearchParams,
    ) -> Result<GitHubSearchResult, IntegrationError> {
        panic!("policy must reject before calling the service")
    }
}

#[tokio::test]
async fn policy_denial_happens_before_the_integration_service_is_called() {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    adapter.push_inbound(inbound("/github user octocat"));

    let services = IntegrationCommandServices {
        github: Some(Arc::new(PanicGitHub)),
        ..IntegrationCommandServices::default()
    };
    let registry = build_registry_with_dependencies(RegistryBuildDependencies {
        basic: BasicCommandsConfig {
            ping: false,
            echo: false,
            coin: false,
            dice: false,
            luck: false,
            twenty_four: false,
            weather: false,
        },
        media: MediaCommandsConfig::all_disabled(),
        web: WebCommandsConfig::all_disabled(),
        third_party: ThirdPartyCommandsConfig {
            github: true,
            ..ThirdPartyCommandsConfig::all_disabled()
        },
        runtime_info: Arc::new(RuntimeInfo::new("test")),
        twenty_four: None,
        controls: ControlCommands::default(),
        services,
        media_services: MediaCommandServices::default(),
        web_services: WebCommandServices::default(),
    })
    .unwrap();
    let policy = PolicyEngine::new(vec![PolicyRule {
        id: "disable-github".to_owned(),
        priority: 0,
        when: PolicyMatch::default(),
        disable_commands: BTreeSet::from(["github".to_owned()]),
        allow_values: Default::default(),
    }])
    .unwrap();
    let handler =
        Arc::new(CommandMessageHandler::new("/", Arc::new(registry), Arc::new(policy)).unwrap());
    let runtime = AppRuntime::builder(
        RuntimeSettings {
            inbound_capacity: 4,
            outbound_capacity: 4,
            handler_timeout: Duration::from_secs(2),
            send_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(2),
        },
        handler,
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_platform(adapter.clone())
    .unwrap()
    .build();

    let cancel = CancellationToken::new();
    let shutdown = cancel.clone();
    let task =
        tokio::spawn(async move { runtime.run(async move { shutdown.cancelled().await }).await });

    adapter.wait_for_sends(1).await;
    assert!(text_of(&adapter.sent()[0]).contains("当前聊天不允许"));

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");
}
