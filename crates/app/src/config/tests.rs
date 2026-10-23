use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::PathBuf;

use lukosbot_domain::PlatformKind;

use super::*;

fn write_config(contents: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

fn load_with(
    path: PathBuf,
    env: &[(&str, &str)],
    overrides: CliOverrides,
) -> Result<LoadedConfig, ConfigError> {
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
        overrides,
    })
}

const ENABLED_TELEGRAM: &str = r#"
    [platforms.telegram]
    enabled = true
    token = "123:fake-token"
    "#;

#[test]
fn missing_file_uses_defaults_and_check_config_does_not_require_a_platform() {
    let loaded = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[],
        CliOverrides::default(),
    )
    .unwrap();

    assert_eq!(loaded.config.runtime.inbound_queue_capacity, 256);
    assert!(loaded.config.require_platform().is_err());
}

#[test]
fn missing_file_with_enabled_platform_uses_defaults() {
    let loaded = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[
            (
                "LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN",
                "123:from-environment",
            ),
            ("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true"),
        ],
        CliOverrides::default(),
    )
    .unwrap();

    assert_eq!(loaded.config.runtime.inbound_queue_capacity, 256);
    assert!(loaded.config.platforms.telegram.enabled);
    assert_eq!(
        loaded.config.platforms.telegram.token(),
        Some("123:from-environment")
    );
}

#[test]
fn file_values_override_defaults() {
    let file = write_config(
        r#"
        [app]
        environment = "production"

        [runtime]
        inbound_queue_capacity = 10

        [platforms.telegram]
        enabled = true
        token = "1:file"
        "#,
    );

    let loaded = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap();

    assert_eq!(loaded.config.app.environment, Environment::Production);
    assert_eq!(loaded.config.runtime.inbound_queue_capacity, 10);
}

#[test]
fn environment_overrides_file() {
    let file = write_config(
        r#"
        [runtime]
        inbound_queue_capacity = 10
        "#,
    );

    let loaded = load_with(
        file.path().to_owned(),
        &[
            ("LUKOSBOT__RUNTIME__INBOUND_QUEUE_CAPACITY", "7"),
            ("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true"),
            ("LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN", "1:env-token"),
        ],
        CliOverrides::default(),
    )
    .unwrap();

    assert_eq!(loaded.config.runtime.inbound_queue_capacity, 7);
}

#[test]
fn cli_log_filter_overrides_environment() {
    let loaded = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[
            ("LUKOSBOT__LOGGING__FILTER", "debug"),
            ("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true"),
            ("LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN", "1:token"),
        ],
        CliOverrides {
            log_filter: Some("warn".to_owned()),
        },
    )
    .unwrap();

    assert_eq!(loaded.config.logging.filter, "warn");
}

#[test]
fn unknown_fields_are_rejected() {
    let file = write_config(
        r#"
        [app]
        environment = "development"
        typo_field = 3

        [platforms.telegram]
        enabled = true
        token = "1:token"
        "#,
    );

    let error = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap_err();
    assert!(matches!(error, ConfigError::Load(_)));
}

#[test]
fn enabled_platform_requires_token() {
    let error = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true")],
        CliOverrides::default(),
    )
    .unwrap_err();

    assert!(
        error
            .issues()
            .iter()
            .any(|issue| issue.contains("token is required"))
    );
}

#[test]
fn enabled_platform_must_be_compiled_in() {
    let file = write_config(ENABLED_TELEGRAM);
    let error = load(LoadOptions {
        path: file.path().to_owned(),
        env: HashMap::new(),
        compiled: CompiledPlatforms {
            telegram: false,
            discord: false,
        },
        overrides: CliOverrides::default(),
    })
    .unwrap_err();

    assert!(
        error
            .issues()
            .iter()
            .any(|issue| issue.contains("build feature"))
    );
}

#[test]
fn numeric_token_from_environment_is_accepted() {
    let loaded = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[
            ("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true"),
            ("LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN", "12345"),
        ],
        CliOverrides::default(),
    )
    .unwrap();

    assert_eq!(loaded.config.platforms.telegram.token(), Some("12345"));
}

#[test]
fn invalid_ranges_are_all_reported() {
    let file = write_config(
        r#"
        [runtime]
        inbound_queue_capacity = 0
        outbound_queue_capacity = 0

        [platforms.telegram]
        enabled = true
        token = "1:token"
        "#,
    );

    let error = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap_err();
    let issues = error.issues();

    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("inbound_queue_capacity"))
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("outbound_queue_capacity"))
    );
}

#[test]
fn secrets_are_redacted_in_debug_output() {
    let file = write_config(
        r#"
        [platforms.telegram]
        enabled = true
        token = "super-secret-token"
        "#,
    );

    let loaded = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap();
    let debug = format!("{:?}", loaded.config);

    assert!(!debug.contains("super-secret-token"));
    assert!(debug.contains("REDACTED"));
}

#[test]
fn summary_does_not_contain_secrets() {
    let file = write_config(
        r#"
        [platforms.telegram]
        enabled = true
        token = "super-secret-token"
        "#,
    );

    let loaded = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap();
    let summary = ConfigSummary(&loaded.config).to_string();

    assert!(!summary.contains("super-secret-token"));
    assert!(summary.contains("telegram=true"));
}

#[test]
fn command_defaults_load_without_a_commands_section() {
    let loaded = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[],
        CliOverrides::default(),
    )
    .unwrap();

    assert_eq!(loaded.config.commands.prefix, "/");
    assert!(loaded.config.commands.basic.ping);
    assert!(loaded.config.commands.basic.twenty_four);
    assert_eq!(loaded.config.commands.twenty_four.time_limit_secs, 300);
}

#[test]
fn environment_overrides_commands() {
    let loaded = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[
            ("LUKOSBOT__COMMANDS__PREFIX", "!"),
            ("LUKOSBOT__COMMANDS__BASIC__DICE", "false"),
            ("LUKOSBOT__COMMANDS__TWENTY_FOUR__TIME_LIMIT_SECS", "5"),
        ],
        CliOverrides::default(),
    )
    .unwrap();

    assert_eq!(loaded.config.commands.prefix, "!");
    assert!(!loaded.config.commands.basic.dice);
    assert_eq!(loaded.config.commands.twenty_four.time_limit_secs, 5);
}

#[test]
fn invalid_command_settings_are_reported() {
    let file = write_config(
        r#"
        [commands]
        prefix = "   "

        [commands.twenty_four]
        time_limit_secs = 0
        "#,
    );

    let error = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap_err();
    let issues = error.issues();

    assert!(issues.iter().any(|issue| issue.contains("commands.prefix")));
    assert!(issues.iter().any(|issue| issue.contains("time_limit_secs")));
}

#[test]
fn unknown_command_fields_are_rejected() {
    let file = write_config(
        r#"
        [commands.basic]
        whois = true
        "#,
    );

    assert!(load_with(file.path().to_owned(), &[], CliOverrides::default()).is_err());
}

#[test]
fn security_and_policy_are_compiled_at_startup() {
    let file = write_config(
        r#"
        [security.bootstrap_bot_admins]
        telegram = [123, 123]
        discord = [456]

        [[policy.rules]]
        id = "telegram-public"
        priority = 100
        disable_commands = ["Kemono", " kemono "]

        [policy.rules.when]
        platform = "Telegram"
        private_chat = false

        [policy.rules.allow_values]
        "[e621.Rating]" = ["S", "q"]
        "#,
    );

    let loaded = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap();
    let admins = loaded.config.security.compile().unwrap();
    assert_eq!(admins.get(&PlatformKind::Telegram).unwrap().len(), 1);

    let rules = loaded.config.policy.compile().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].when.platform, Some(PlatformKind::Telegram));
    assert_eq!(
        rules[0].disable_commands,
        BTreeSet::from(["kemono".to_owned()])
    );
    assert_eq!(
        rules[0].allow_values.get("[e621.rating]").unwrap(),
        &BTreeSet::from(["s".to_owned(), "q".to_owned()])
    );
}

#[test]
fn invalid_security_and_policy_are_reported_as_config_issues() {
    let file = write_config(
        r#"
        [security.bootstrap_bot_admins]
        unknown = [1]

        [[policy.rules]]
        id = "bad-platform"
        [policy.rules.when]
        platform = "matrix"

        [[policy.rules]]
        id = "duplicate"
        [policy.rules.allow_values]
        "A" = ["s"]
        "a" = ["q"]
        "#,
    );

    let error = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap_err();
    let issues = error.issues();
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("unknown platform"))
    );
    assert!(issues.iter().any(|issue| issue.contains("matrix")));
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("duplicate normalized"))
    );
}

#[test]
fn expiry_cleanup_can_be_disabled_or_bounded() {
    let file = write_config(
        r#"
        [storage]
        expiry_cleanup_interval_secs = 0
        "#,
    );
    assert!(load_with(file.path().to_owned(), &[], CliOverrides::default()).is_ok());

    let file = write_config(
        r#"
        [storage]
        expiry_cleanup_interval_secs = 86401
        "#,
    );
    let error = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap_err();
    assert!(
        error
            .issues()
            .iter()
            .any(|issue| issue.contains("expiry_cleanup_interval_secs"))
    );
}

#[test]
fn control_commands_load_from_environment() {
    let loaded = load_with(
        PathBuf::from("/definitely/missing.toml"),
        &[
            ("LUKOSBOT__COMMANDS__CONTROL__ADMIN", "false"),
            ("LUKOSBOT__COMMANDS__CONTROL__PREF", "false"),
        ],
        CliOverrides::default(),
    )
    .unwrap();

    assert!(!loaded.config.commands.control.admin);
    assert!(!loaded.config.commands.control.pref);
    assert!(loaded.config.commands.control.service);
}
