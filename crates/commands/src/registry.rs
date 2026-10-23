use std::sync::Arc;

use lukosbot_command_engine::{CommandRegistry, CommandRegistryBuilder, CommandRegistryError};

use crate::basic;
use crate::basic::random::{RandomSource, ThreadRandom};
use crate::basic::twenty_four::TwentyFourHandle;
use crate::config::BasicCommandsConfig;
use crate::runtime_info::RuntimeInfo;

/// Builds the built-in command registry.
///
/// `/start` and `/help` are always registered. Other commands are controlled by
/// [`BasicCommandsConfig`].
pub fn build_registry(
    config: &BasicCommandsConfig,
    runtime_info: Arc<RuntimeInfo>,
    twenty_four: Option<TwentyFourHandle>,
) -> Result<CommandRegistry, CommandRegistryError> {
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
            });
        };
        builder.register(basic::twenty_four::command::definition(handle))?;
    }
    if config.weather {
        builder.register(basic::weather::definition())?;
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
}
