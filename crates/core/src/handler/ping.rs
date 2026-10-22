use async_trait::async_trait;

use lukosbot_domain::{InboundMessage, OutboundMessage};

use crate::handler::{HandlerContext, HandlerError, MessageHandler};

/// A deliberately small smoke handler that proves the inbound/outbound pipeline end to end.
///
/// This is not a command engine: it recognises a single `ping` word and replies `pong`.
#[derive(Debug, Default, Clone, Copy)]
pub struct PingHandler;

#[async_trait]
impl MessageHandler for PingHandler {
    async fn handle(
        &self,
        context: &HandlerContext,
        message: InboundMessage,
    ) -> Result<(), HandlerError> {
        let is_ping = message
            .text_parts()
            .any(|text| text.trim().eq_ignore_ascii_case("ping"));

        if !is_ping {
            return Ok(());
        }

        context
            .outbound()
            .send(message.address.clone(), OutboundMessage::text("pong"))
            .await?;

        Ok(())
    }
}
