# ADR-0002: Runtime, queues, and shutdown

- Status: accepted
- Date: 2026-10-23

## Context

Bot workloads combine long-lived network loops, user-triggered work, bounded external APIs, and a
process that must stop on `SIGINT`/`SIGTERM`. Unbounded channels and detached `tokio::spawn` calls
are the usual sources of memory growth and hangs during shutdown.

## Decision

### Supervision

`AppRuntime` owns a `JoinSet` containing every long-running task: one task per adapter, the
dispatcher, the outbound worker, and any registered `RuntimeWorker` (added in ADR-0004). Tasks
return `Result<(), TaskFailure>`. An `Ok(())` outside cancellation or a panic is treated as a fatal
condition: the runtime cancels all tasks, performs the shutdown sequence, and returns
`RuntimeError::TaskFailed`.

### Queues and ordering

- Inbound and outbound queues are bounded `tokio::sync::mpsc` channels configured by
  `runtime.*_queue_capacity`.
- Adapters await `send` on the inbound queue. If a platform SDK dispatches events from an async
  context (both do here), this propagates backpressure instead of dropping messages silently.
- A single dispatcher processes messages in order and enforces `runtime.handler_timeout_secs`. A
  handler failure or timeout is logged and counted; it does not stop the dispatcher.
- A single outbound worker resolves adapters by platform, applies `runtime.send_timeout_secs`, and
  returns a `oneshot` result to the caller. It is serial by design; per-chat concurrency and
  per-platform workers are deferred until measurements justify their ordering and lifecycle
  complexity.

### Shutdown

Cancellation uses `tokio_util::sync::CancellationToken`. The runtime waits for the dispatcher to
finish the current message before closing the outbound channel, so work accepted before shutdown can
still produce a reply. Then it drains outbound work, calls every `PlatformAdapter::shutdown`
concurrently (idempotent), closes the state store, and aborts whatever remains when the shared
deadline expires.

`PlatformAdapter::run` returns `Ok(())` only on cancellation. Any other termination is an error, so
the runtime never pretends a disconnected platform is healthy.

## Consequences

- Tests can drive the whole pipeline with `RecordingAdapter` and `MemoryStateStore`.
- A slow handler delays the queue, but it cannot grow it without bound.
- A hard shutdown deadline is observable (`ShutdownReport::timed_out`) instead of hanging.
- Throughput is limited to one message at a time until a concurrency design is accepted.
