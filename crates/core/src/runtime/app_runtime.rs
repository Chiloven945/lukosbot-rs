use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use lukosbot_domain::PlatformKind;

use crate::error::TaskFailure;
use crate::handler::{HandlerContext, MessageHandler};
use crate::messaging::dispatcher::Dispatcher;
use crate::messaging::outbound::{AdapterRegistry, OutboundHandle, OutboundService};
use crate::ports::{PlatformAdapter, StateStore};
use crate::runtime::metrics::{MetricsSnapshot, RuntimeMetrics};
use crate::runtime::task_supervisor::TaskSupervisor;
use crate::runtime::worker::{RuntimeWorker, WorkerContext};

const RESERVED_TASK_NAMES: [&str; 2] = ["dispatcher", "outbound"];

/// Runtime limits derived from validated configuration.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeSettings {
    pub inbound_capacity: usize,
    pub outbound_capacity: usize,
    pub handler_timeout: Duration,
    pub send_timeout: Duration,
    pub shutdown_timeout: Duration,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            inbound_capacity: 256,
            outbound_capacity: 128,
            handler_timeout: Duration::from_secs(30),
            send_timeout: Duration::from_secs(20),
            shutdown_timeout: Duration::from_secs(15),
        }
    }
}

/// Builds an [`AppRuntime`] with its adapters and dependencies.
pub struct RuntimeBuilder {
    settings: RuntimeSettings,
    handler: Arc<dyn MessageHandler>,
    store: Arc<dyn StateStore>,
    adapters: Vec<Arc<dyn PlatformAdapter>>,
    workers: Vec<Arc<dyn RuntimeWorker>>,
    worker_names: HashSet<&'static str>,
    metrics: Arc<RuntimeMetrics>,
}

impl RuntimeBuilder {
    /// Registers a platform adapter. Registering the same platform twice is an error.
    pub fn add_platform(mut self, adapter: Arc<dyn PlatformAdapter>) -> Result<Self, RuntimeError> {
        let platform = adapter.kind();
        if self
            .adapters
            .iter()
            .any(|existing| existing.kind() == platform)
        {
            return Err(RuntimeError::DuplicatePlatform(platform));
        }
        if self.worker_names.contains(platform.as_str()) {
            return Err(RuntimeError::DuplicateTaskName(platform.as_str()));
        }
        self.adapters.push(adapter);
        Ok(self)
    }

    /// Registers a supervised long-lived worker.
    pub fn add_worker(mut self, worker: Arc<dyn RuntimeWorker>) -> Result<Self, RuntimeError> {
        let name = worker.name();
        if RESERVED_TASK_NAMES.contains(&name)
            || self.worker_names.contains(name)
            || self
                .adapters
                .iter()
                .any(|adapter| adapter.kind().as_str() == name)
        {
            return Err(RuntimeError::DuplicateTaskName(name));
        }

        self.worker_names.insert(name);
        self.workers.push(worker);
        Ok(self)
    }

    pub fn with_metrics(mut self, metrics: Arc<RuntimeMetrics>) -> Self {
        self.metrics = metrics;
        self
    }

    pub fn build(self) -> AppRuntime {
        AppRuntime {
            settings: self.settings,
            handler: self.handler,
            store: self.store,
            adapters: self.adapters,
            workers: self.workers,
            metrics: self.metrics,
        }
    }
}

pub struct AppRuntime {
    settings: RuntimeSettings,
    handler: Arc<dyn MessageHandler>,
    store: Arc<dyn StateStore>,
    adapters: Vec<Arc<dyn PlatformAdapter>>,
    workers: Vec<Arc<dyn RuntimeWorker>>,
    metrics: Arc<RuntimeMetrics>,
}

impl AppRuntime {
    pub fn builder(
        settings: RuntimeSettings,
        handler: Arc<dyn MessageHandler>,
        store: Arc<dyn StateStore>,
    ) -> RuntimeBuilder {
        RuntimeBuilder {
            settings,
            handler,
            store,
            adapters: Vec::new(),
            workers: Vec::new(),
            worker_names: HashSet::new(),
            metrics: Arc::new(RuntimeMetrics::default()),
        }
    }

    pub fn metrics(&self) -> Arc<RuntimeMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Runs until `shutdown` resolves or a supervised task fails, then shuts down in a bounded,
    /// ordered fashion and returns a report.
    pub async fn run(
        self,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<ShutdownReport, RuntimeError> {
        let AppRuntime {
            settings,
            handler,
            store,
            adapters,
            workers,
            metrics,
        } = self;

        if adapters.is_empty() {
            return Err(RuntimeError::NoPlatforms);
        }

        let registry: AdapterRegistry = adapters
            .iter()
            .map(|adapter| (adapter.kind(), Arc::clone(adapter)))
            .collect();

        let cancel = CancellationToken::new();
        let mut supervisor = TaskSupervisor::new();

        let (inbound_sender, inbound_receiver) = mpsc::channel(settings.inbound_capacity);
        let (outbound, worker) = OutboundService::start(
            registry,
            settings.outbound_capacity,
            settings.send_timeout,
            Arc::clone(&metrics),
        );

        let context = Arc::new(HandlerContext::new(
            outbound.clone(),
            Arc::clone(&store),
            cancel.clone(),
        ));

        let dispatcher = Dispatcher::new(
            inbound_receiver,
            Arc::clone(&handler),
            context,
            settings.handler_timeout,
            Arc::clone(&metrics),
            cancel.clone(),
        );

        supervisor.spawn("dispatcher", async move {
            dispatcher.run().await;
            Ok(())
        });
        supervisor.spawn("outbound", async move {
            worker.run().await;
            Ok(())
        });

        for worker in &workers {
            let worker = Arc::clone(worker);
            let context = WorkerContext::new(outbound.clone(), cancel.child_token());
            supervisor.spawn(worker.name(), async move { worker.run(context).await });
        }

        for adapter in &adapters {
            let adapter = Arc::clone(adapter);
            let platform = adapter.kind();
            let inbound = inbound_sender.clone();
            let cancel = cancel.child_token();

            supervisor.spawn(platform.as_str(), async move {
                adapter.run(inbound, cancel).await.map_err(|error| {
                    TaskFailure::new(format!("{platform} adapter stopped with an error"))
                        .with_source(error)
                })
            });
        }

        // The runtime itself must not keep an inbound sender alive, otherwise the dispatcher
        // cannot observe that all adapters are gone.
        drop(inbound_sender);

        let mut fatal: Option<TaskFailure> = None;
        tokio::pin!(shutdown);

        tokio::select! {
            biased;

            _ = &mut shutdown => {
                info!("shutdown requested");
                cancel.cancel();
            }

            joined = supervisor.join_next() => {
                match joined {
                    Some(Ok(outcome)) => {
                        let failure = match outcome.result {
                            Err(error) => error,
                            Ok(()) => TaskFailure::new(format!(
                                "supervised task `{}` exited unexpectedly",
                                outcome.name
                            )),
                        };
                        metrics.task_failures.fetch_add(1, Ordering::Relaxed);
                        fatal = Some(failure);
                        cancel.cancel();
                    }
                    Some(Err(error)) => {
                        metrics.task_failures.fetch_add(1, Ordering::Relaxed);
                        fatal = Some(TaskFailure::new(format!(
                            "supervised task panicked: {error}"
                        )));
                        cancel.cancel();
                    }
                    None => {
                        if !cancel.is_cancelled() {
                            metrics.task_failures.fetch_add(1, Ordering::Relaxed);
                            fatal = Some(TaskFailure::new(
                                "all supervised tasks stopped unexpectedly",
                            ));
                            cancel.cancel();
                        }
                    }
                }
            }
        }

        let report = shutdown_all(
            supervisor,
            &adapters,
            outbound,
            store,
            settings.shutdown_timeout,
            &metrics,
        )
        .await;

        match fatal {
            Some(error) => Err(RuntimeError::TaskFailed(error)),
            None => Ok(report),
        }
    }
}

async fn shutdown_all(
    mut supervisor: TaskSupervisor,
    adapters: &[Arc<dyn PlatformAdapter>],
    outbound: OutboundHandle,
    store: Arc<dyn StateStore>,
    timeout: Duration,
    metrics: &RuntimeMetrics,
) -> ShutdownReport {
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let mut abandoned_tasks = 0;
    let mut outbound = Some(outbound);

    // Wait for the dispatcher (and adapters) to finish. The outbound handle is closed only after
    // the dispatcher has stopped, so messages accepted before shutdown can still be delivered.
    while supervisor.len() > 0 {
        match timeout_at(deadline, supervisor.join_next()).await {
            Ok(Some(Ok(outcome))) => {
                if outcome.name == "dispatcher" {
                    close_outbound(&mut outbound);
                }
                if let Err(error) = outcome.result {
                    debug!(task = outcome.name, %error, "task ended with an error during shutdown");
                }
            }
            Ok(Some(Err(error))) => {
                debug!(%error, "task join error during shutdown");
            }
            Ok(None) => {
                close_outbound(&mut outbound);
                break;
            }
            Err(_) => {
                timed_out = true;
                abandoned_tasks = supervisor.abort_all();
                close_outbound(&mut outbound);
                warn!(
                    abandoned_tasks,
                    "supervised tasks did not stop before the deadline"
                );
                break;
            }
        }
    }
    close_outbound(&mut outbound);

    if shutdown_adapters(adapters, deadline).await {
        timed_out = true;
    }

    match timeout_at(deadline, store.close()).await {
        Ok(Ok(())) => debug!("state store closed"),
        Ok(Err(error)) => warn!(%error, "state store close failed"),
        Err(_) => {
            timed_out = true;
            warn!("state store close timed out");
        }
    }

    let metrics = metrics.snapshot();
    info!(
        inbound_received = metrics.inbound_received,
        inbound_dropped = metrics.inbound_dropped,
        handled_ok = metrics.handled_ok,
        handled_failed = metrics.handled_failed,
        outbound_sent = metrics.outbound_sent,
        outbound_failed = metrics.outbound_failed,
        "runtime stopped"
    );

    ShutdownReport {
        metrics,
        abandoned_tasks,
        timed_out,
    }
}

fn close_outbound(handle: &mut Option<OutboundHandle>) {
    if let Some(handle) = handle.take() {
        handle.close();
    }
}

async fn shutdown_adapters(adapters: &[Arc<dyn PlatformAdapter>], deadline: Instant) -> bool {
    let mut tasks = JoinSet::new();
    for adapter in adapters {
        let adapter = Arc::clone(adapter);
        tasks.spawn(async move {
            let platform = adapter.kind();
            (platform, adapter.shutdown().await)
        });
    }

    while !tasks.is_empty() {
        match timeout_at(deadline, tasks.join_next()).await {
            Ok(Some(Ok((platform, Ok(()))))) => {
                debug!(%platform, "platform adapter shut down");
            }
            Ok(Some(Ok((platform, Err(error))))) => {
                warn!(%platform, %error, "platform adapter shutdown failed");
            }
            Ok(Some(Err(error))) => {
                warn!(%error, "platform adapter shutdown task failed");
            }
            Ok(None) => break,
            Err(_) => {
                tasks.abort_all();
                warn!("platform adapter shutdown timed out");
                return true;
            }
        }
    }

    false
}

/// What the runtime did while shutting down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownReport {
    pub metrics: MetricsSnapshot,
    pub abandoned_tasks: usize,
    pub timed_out: bool,
}

/// A runtime assembly or execution failure.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("no platform adapters are registered")]
    NoPlatforms,

    #[error("a platform adapter for {0} is already registered")]
    DuplicatePlatform(PlatformKind),

    #[error("a runtime task named `{0}` is already registered")]
    DuplicateTaskName(&'static str),

    #[error(transparent)]
    TaskFailed(#[from] TaskFailure),
}
