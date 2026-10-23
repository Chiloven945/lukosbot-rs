use std::io::Write;
use std::path::PathBuf;

use super::*;

fn load_with(path: PathBuf, env: &[(&str, &str)]) -> Result<LoadedConfig, ConfigError> {
    load(LoadOptions {
        path,
        env: env
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
        compiled: CompiledPlatforms {
            telegram: true,
            discord: true,
        },
        overrides: CliOverrides::default(),
    })
}

fn write_config(contents: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

#[test]
fn integration_defaults_are_enabled_and_unconfigured() {
    let loaded = load_with(PathBuf::from("/definitely/missing.toml"), &[]).unwrap();

    assert!(loaded.config.commands.integrations.github);
    assert!(loaded.config.commands.integrations.bilibili);
    assert!(loaded.config.commands.integrations.ip);
    assert!(loaded.config.commands.integrations.music);
    assert!(loaded.config.commands.integrations.whois);
    assert!(!loaded.config.integrations.github.has_token());
    assert!(!loaded.config.integrations.music.spotify.available());
    assert!(!loaded.config.integrations.music.soundcloud.available());
    assert_eq!(loaded.config.integrations.whois.connect_timeout_secs, 3);
    assert_eq!(loaded.config.integrations.whois.total_timeout_secs, 8);
}

#[test]
fn integration_environment_and_legacy_github_token_are_supported() {
    let loaded = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[
            ("LUKOSBOT__INTEGRATIONS__MUSIC__SPOTIFY__ENABLED", "true"),
            (
                "LUKOSBOT__INTEGRATIONS__MUSIC__SPOTIFY__CLIENT_ID",
                "spotify-id",
            ),
            (
                "LUKOSBOT__INTEGRATIONS__MUSIC__SPOTIFY__CLIENT_SECRET",
                "spotify-secret",
            ),
            ("LUKOSBOT__INTEGRATIONS__MUSIC__SOUNDCLOUD__ENABLED", "true"),
            (
                "LUKOSBOT__INTEGRATIONS__MUSIC__SOUNDCLOUD__CLIENT_ID",
                "soundcloud-id",
            ),
            ("GITHUB_TOKEN", "legacy-token"),
        ],
    )
    .unwrap();

    assert!(loaded.config.integrations.music.spotify.available());
    assert!(loaded.config.integrations.music.soundcloud.available());
    assert_eq!(
        loaded.config.integrations.github.token(),
        Some("legacy-token")
    );
}

#[test]
fn the_new_github_environment_variable_wins_over_the_legacy_one() {
    let loaded = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[
            ("LUKOSBOT__INTEGRATIONS__GITHUB__TOKEN", "new-token"),
            ("GITHUB_TOKEN", "legacy-token"),
        ],
    )
    .unwrap();

    assert_eq!(loaded.config.integrations.github.token(), Some("new-token"));
}

#[test]
fn whois_timeout_ranges_are_validated() {
    let file = write_config(
        r#"
        [integrations.whois]
        connect_timeout_secs = 10
        total_timeout_secs = 5
        max_response_bytes = 10
        "#,
    );
    let error = load_with(file.path().to_owned(), &[]).unwrap_err();
    let issues = error.issues();

    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("max_response_bytes"))
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("total_timeout_secs"))
    );
}

#[test]
fn integration_secrets_are_redacted_from_summary_and_debug() {
    let file = write_config(
        r#"
        [integrations.github]
        token = "github-secret"

        [integrations.music.spotify]
        enabled = true
        client_id = "spotify-id-secret"
        client_secret = "spotify-client-secret"

        [integrations.music.soundcloud]
        enabled = true
        client_id = "soundcloud-secret"
        "#,
    );
    let loaded = load_with(file.path().to_owned(), &[]).unwrap();
    let summary = ConfigSummary(&loaded.config).to_string();
    let debug = format!("{:?}", loaded.config);

    assert!(summary.contains("github_token=configured"));
    assert!(summary.contains("spotify=configured"));
    assert!(summary.contains("soundcloud=configured"));
    for secret in [
        "github-secret",
        "spotify-id-secret",
        "spotify-client-secret",
        "soundcloud-secret",
    ] {
        assert!(!summary.contains(secret));
        assert!(!debug.contains(secret));
    }
}

#[test]
fn example_configuration_is_valid() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/application.example.toml");
    let loaded = load_with(path, &[]).unwrap();
    assert!(loaded.config.commands.integrations.github);
    assert!(!loaded.config.integrations.music.spotify.available());
}
