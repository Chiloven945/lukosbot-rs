# ADR-0004: Command engine, parsing, and supervised workers

- Status: accepted
- Date: 2026-10-23

## Context

The runtime foundation already provides a platform-independent `MessageHandler`, bounded queues, and
a supervised lifecycle. The next step is a command layer that can be extended with JVM-compatible
commands without coupling command code to Telegram or Discord SDKs or recreating the runtime.

## Decision

### Crate boundaries

- `lukosbot-command-engine` owns command names, definitions, input specs, tokenization, argv
  parsing, the registry, execution context, the message handler, and text help.
- `lukosbot-commands` owns concrete commands and the 24-point actor.
- `lukosbot-core` remains unaware of commands. It only gains `RuntimeWorker` and `WorkerContext`.
- `lukosbot-app` is the sole composition root; it creates the registry, handler, workers, adapters,
  and runtime.

Concrete commands never depend on platform SDKs. They reply through `CommandContext` and the core
outbound handle.

### Parsing compatibility

The engine mirrors the JVM parser rather than reusing `clap`:

- quote-aware tokenization without shell expansion;
- `--name=value`, `--name value`, `-n value`, boolean flags, and `--`;
- negative integers are values, not options;
- `-abc` is a positional;
- non-repeatable options use last-wins; repeatable options accumulate;
- `split_by` lists are validated per item;
- greedy positionals are unique and last.

`InputSpec::Raw` never runs the tokenizer. This is required for `/echo` and `/24`, where an
unmatched quote or operator must reach the command executor unchanged.

Command paths are resolved with a whitespace-only literal scan. This deliberately avoids treating a
raw argument as structured input during resolution.

### Validation and errors

Definitions are validated at startup. Duplicate names, invalid aliases, conflicting child names, bad
option names, duplicate argument names, required/default conflicts, and malformed greedy positionals
fail assembly. This is an intentional change from the JVM registry's warning-only behaviour.

Low-level parser errors are typed. The handler renders them uniformly as a localized error plus a
help hint. Unknown root commands are ignored, matching the JVM bot and avoiding replies to ordinary
chat text.

### Text help

Help is generated from registry metadata. The help executor accesses the registry through
`CommandContext` rather than the registry holding a back-reference to the help command. Hidden
commands are excluded from both the list and detail lookup. Image help is deferred.

### Runtime workers and `/24`

The 24-point command needs a timer that can send a message after the original command invocation has
finished. The initial suggested API, `run(cancel)`, cannot satisfy that requirement. The accepted
API is:

```rust
#[async_trait]
pub trait RuntimeWorker: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(&self, context: WorkerContext) -> Result<(), TaskFailure>;
}
```

`WorkerContext` contains the outbound handle and cancellation token. Workers are registered through
`RuntimeBuilder::add_worker`, receive unique non-reserved names, are spawned by the existing
`TaskSupervisor`, and share the shutdown deadline. They are not detached tasks.

`/24` uses one actor to own all game sessions. The actor waits on events, cancellation, and the
nearest deadline with `tokio::select!`; it removes expired sessions and sends solutions back to the
stored chat address. Sessions are keyed by platform + user ID and are intentionally in-memory.

## Consequences

- Parser and registry behaviour can be tested without tokens or network access.
- Raw command semantics remain stable while structured commands share one parser.
- `/24` timeout delivery is supervised and bounded by normal shutdown.
- The worker API is slightly larger than the first sketch, but the extra outbound handle is
  necessary for correct timeout notifications.
- JVM compatibility is explicit and documented; differences are recorded in
  `docs/command-engine/behavior-matrix.md`.
