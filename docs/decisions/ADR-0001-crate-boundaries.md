# ADR-0001: Crate boundaries

- Status: accepted
- Date: 2026-10-23

## Context

The JVM implementation is a large, feature-rich bot. The Rust rewrite starts from an empty
repository and must not arrive at a structure shaped by Gradle modules or by the first commands that
happen to be ported. The foundation needs boundaries that are stable enough for a command engine,
policy system, media pipeline, and future storage backends, while remaining small enough to build
and test quickly.

## Decision

Use a Cargo workspace with six crates:

- `lukosbot-domain` — platform-independent types. No SDK, Tokio, SQLx, or HTTP.
- `lukosbot-core` — ports (`PlatformAdapter`, `StateStore`), dispatch, outbound routing, runtime.
- `lukosbot-infra` — SQLite and the shared HTTP client.
- `lukosbot-platform-telegram`, `lukosbot-platform-discord` — adapters.
- `lukosbot-app` — CLI, configuration, composition root, process lifecycle.

The dependency graph is acyclic and directed towards the domain. Ports live in `core` rather than a
separate `ports` crate: today only the core application consumes them, and a new crate must earn its
maintenance cost.

`async_trait` is used only for the object-safe ports that the composition root stores as
`Arc<dyn Trait>`. Internal code uses concrete types. Domain types are not generic over platforms and
never wrap SDK objects.

Configuration and telemetry remain modules of `lukosbot-app`. A `lukosbot-command-engine` crate is
deferred until a real parser subsystem exists.

## Consequences

- Adapters and infrastructure can be tested with fakes without starting the application.
- `core` cannot depend on SQLx, Reqwest, Teloxide, or Serenity, which keeps handler tests fast.
- Adding a platform means adding a crate and a feature in the application, not editing `core`.
- A future reusable application (for example, a second binary) may justify extracting ports or
  configuration; that requires a new ADR rather than speculative splitting.
