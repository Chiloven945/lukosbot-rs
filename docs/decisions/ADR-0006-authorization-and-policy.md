# ADR-0006: Authorization and policy

- Status: accepted
- Date: 2026-10-23

## Context

The JVM bot has two independent authority mechanisms: administrator identity (global bot admins and
per-chat admins) and command policy (platform/chat/NSFW/ID conditions, disabled commands, allowed
values). The Rust foundation currently has neither, and the command engine has no policy gate.

## Decision

### Authorization

`core::auth` owns:

- `BotAdminService`: bootstrap administrators from configuration plus dynamic records at
  `GLOBAL / auth / bot_admins`;
- `AuthorizationService`: combines bot-admin state with `ChatAdminResolver` implementations;
- `AuthContext`: an immutable per-message snapshot with `can_manage_global` and `can_manage_chat`.

Bootstrap and dynamic administrators are unioned. Dynamic storage accepts the legacy JVM JSON shape
(uppercase platform keys, numeric IDs) but writes canonical lowercase keys and string IDs. Dynamic
read-modify-write is serialised with `tokio::sync::Mutex`; this is explicitly a single-process
guarantee.

If no bootstrap administrator exists, removing the last effective dynamic administrator is refused.
Storage corruption or resolver failure fails closed and is distinguishable from "not an
administrator".

Platform chat-admin resolution is asynchronous. Telegram uses `get_chat_member` with a 60-second
cache; Discord records trusted permission snapshots from gateway message events in a bounded cache.
Resolver calls are bounded by a timeout. Missing context is not interpreted as administrator
authority.

### Policy

`core::policy` is immutable and independent of TOML/platform SDKs. A rule matches when all non-empty
conditions match:

- platform, private-chat, group, NSFW, chat ID, user ID.

Matching rules union their disabled commands and intersect allowed-value sets. `priority` provides
stable ordering and diagnostics but does not turn the disabled-command result into
"highest-priority-only".

`MessageContext.nsfw` carries trusted platform context; Telegram marks itself non-NSFW, Discord uses
the known channel state or `None`, and policy treats unknown as non-NSFW.

`CommandMessageHandler` applies policy to the canonical root command before argument parsing.
Aliases therefore cannot bypass policy, and invalid arguments cannot reveal usage for blocked
commands. `/help` uses the same query to filter its list and detail view.

Authorization and policy remain separate: an administrator is not automatically exempt from policy,
and policy does not grant management authority.

## Consequences

- Management commands call `BotAdminService`/`AuthorizationService`; they never compare IDs ad hoc.
- Platform crates implement `ChatAdminResolver` without making `core` depend on their SDK.
- Policy rules are validated and frozen at startup; no TOML is parsed per message.
- `/pref` can reuse the same authorization decisions for user/chat/global scopes.
- Cross-process dynamic-admin updates and distributed policy invalidation remain future work.
