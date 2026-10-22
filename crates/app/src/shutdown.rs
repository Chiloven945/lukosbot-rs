//! Process signal handling.

/// Resolves on the first shutdown signal (`SIGINT` or, on Unix, `SIGTERM`).
///
/// A second signal is intentionally not handled by a detached task: once the runtime owns
/// shutdown, the operating system or process supervisor is the final escalation mechanism.
pub async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => {
                        if let Err(error) = result {
                            tracing::warn!(%error, "could not listen for Ctrl-C");
                        }
                    }
                    _ = terminate.recv() => {}
                }
            }
            Err(error) => {
                tracing::warn!(%error, "could not listen for SIGTERM; falling back to Ctrl-C");
                if let Err(error) = tokio::signal::ctrl_c().await {
                    tracing::warn!(%error, "could not listen for Ctrl-C");
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "could not listen for Ctrl-C");
        }
    }

    tracing::info!("shutdown signal received");
}
