use std::sync::Arc;

use thiserror::Error;

use lukosbot_command_engine::{CommandRegistry, CommandRegistryBuilder, CommandRegistryError};

use crate::basic;
use crate::basic::random::{RandomSource, ThreadRandom};
use crate::basic::twenty_four::TwentyFourHandle;
use crate::config::BasicCommandsConfig;
use crate::control::ControlCommands;
use crate::runtime_info::RuntimeInfo;

/// A failure while assembling the command registry.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CommandBuildError {
    #[error(transparent)]
    Registry(#[from] CommandRegistryError),

    #[error("command `{command}` requires {dependency}")]
    MissingDependency {
        command: &'static str,
        dependency: &'static str,
    },
}

/// Builds the built-in command registry without optional control commands.
pub fn build_registry(
    config: &BasicCommandsConfig,
    runtime_info: Arc<RuntimeInfo>,
    twenty_four: Option<TwentyFourHandle>,
) -> Result<CommandRegistry, CommandBuildError> {
    build_registry_with_controls(
        config,
        runtime_info,
        twenty_four,
        ControlCommands::default(),
    )
}

/// Builds the built-in command registry and, when enabled, `/admin` and `/pref`.
pub fn build_registry_with_controls(
    config: &BasicCommandsConfig,
    runtime_info: Arc<RuntimeInfo>,
    twenty_four: Option<TwentyFourHandle>,
    controls: ControlCommands,
) -> Result<CommandRegistry, CommandBuildError> {
    let mut builder = CommandRegistryBuilder::new();
    let random: Arc<dyn RandomSource> = Arc::new(ThreadRandom);

    if config.ping {
        builder.register(basic::ping::definition(runtime_info))?;
    }
    if config.echo {
        builder.register(basic::echo::definition())?;
    }
    if config.coin {
        builder.register(basic::coin::definition(Arc::clone(&random)))?;
    }
    if config.dice {
        builder.register(basic::dice::definition(Arc::clone(&random)))?;
    }
    if config.luck {
        builder.register(basic::luck::definition())?;
    }
    if config.twenty_four {
        let Some(handle) = twenty_four else {
            return Err(CommandRegistryError::InvalidDefinition {
                path: "24".to_owned(),
                message: "the 24-point command is enabled but no service handle was provided"
                    .to_owned(),
            }
            .into());
        };
        builder.register(basic::twenty_four::command::definition(handle))?;
    }
    if config.weather {
        builder.register(basic::weather::definition())?;
    }

    if controls.admin {
        let admins = controls
            .bot_admins
            .ok_or(CommandBuildError::MissingDependency {
                command: "admin",
                dependency: "BotAdminService",
            })?;
        let authorization =
            controls
                .authorization
                .clone()
                .ok_or(CommandBuildError::MissingDependency {
                    command: "admin",
                    dependency: "AuthorizationService",
                })?;
        builder.register(crate::admin::definition(admins, authorization))?;
    }

    if controls.pref {
        let states = controls
            .state_service
            .ok_or(CommandBuildError::MissingDependency {
                command: "pref",
                dependency: "StateService",
            })?;
        let authorization = controls
            .authorization
            .ok_or(CommandBuildError::MissingDependency {
                command: "pref",
                dependency: "AuthorizationService",
            })?;
        builder.register(crate::pref::definition(states, authorization))?;
    }

    builder.register(basic::start::definition())?;
    builder.register(basic::help::definition())?;

    Ok(builder.build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_commands_are_not_registered_but_start_and_help_remain() {
        let config = BasicCommandsConfig {
            ping: false,
            echo: false,
            coin: false,
            dice: false,
            luck: false,
            twenty_four: false,
            weather: false,
        };
        let registry = build_registry(&config, Arc::new(RuntimeInfo::new("test")), None).unwrap();

        assert!(registry.find("ping").is_none());
        assert!(registry.find("echo").is_none());
        assert!(registry.find("start").is_some());
        assert!(registry.find("help").is_some());
        assert!(registry.resolve("ping").unwrap().is_none());
    }

    #[test]
    fn enabled_twenty_four_requires_a_handle() {
        let config = BasicCommandsConfig {
            twenty_four: true,
            ..BasicCommandsConfig::default()
        };
        assert!(build_registry(&config, Arc::new(RuntimeInfo::new("test")), None).is_err());
    }

    #[test]
    fn enabled_control_command_requires_its_dependencies() {
        let config = BasicCommandsConfig {
            twenty_four: false,
            ..BasicCommandsConfig::default()
        };
        let error = match build_registry_with_controls(
            &config,
            Arc::new(RuntimeInfo::new("test")),
            None,
            ControlCommands {
                admin: true,
                ..ControlCommands::default()
            },
        ) {
            Ok(_) => panic!("admin requires dependencies"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            CommandBuildError::MissingDependency {
                command: "admin",
                ..
            }
        ));
    }
}
