# Command Engine Behaviour Matrix

This document freezes the behaviour implemented by the text command engine and the first basic
command set. It is the reference for future migrations; deviations from the JVM bot are listed
explicitly.

## Entry rules

| Item | Behaviour |
|---|---|
| Prefix | Default `/`; configured through `commands.prefix` |
| Prefix matching | Exact, case-sensitive; the prefix is stripped before command lookup |
| Command names | ASCII case-insensitive |
| Root aliases | Registered on `CommandDefinition`; resolved case-insensitively |
| Child literals | ASCII case-insensitive; child literal wins over a parent fallback executor |
| Unknown root command | Ignored silently |
| Empty command after prefix | Ignored silently |
| Plain chat text | Ignored |
| Known command with bad arguments | Reply with `命令参数错误：…` and a `/help <path>` hint |
| Unknown subcommand | Reply with a localized error and help hint |
| Duplicate command/alias at startup | Registration fails; the application does not start |

## Parser rules

The tokenizer and argv parser intentionally mirror the JVM implementation, including its quirks.

| Input | Behaviour |
|---|---|
| Whitespace | Runs of Unicode whitespace separate tokens |
| `'a b'`, `"a b"` | Quote at token start preserves internal spaces |
| Quote inside a started token | Treated as a literal character |
| Unclosed quote | Parse error with a byte position |
| Empty quoted token | Does not produce an argument (JVM-compatible) |
| `--name=value` | Inline long option value |
| `--name value` | Next token is the value unless it looks like another option |
| `-n value` | Short option; only exactly two characters is an option |
| `-abc` | Positional (JVM-compatible short-option policy) |
| `-1` | Negative integer, not a short option |
| `--flag` | Boolean `true` |
| `--flag=false` | Boolean `false`; other values are rejected |
| `--` | Stops option parsing; later tokens are positional |
| Repeatable option | Accumulates a list |
| Non-repeatable repeated option | Last value wins |
| `split_by` | Splits, drops empty parts, validates each part |
| Greedy positional | Must be unique and last; joins remaining tokens with one space |
| Extra positional | Parse error |
| Missing required argument | Parse error |
| Defaults | Applied only when the user did not provide a value |

`InputSpec::Raw` does not invoke the tokenizer. `/echo` and `/24` therefore preserve arbitrary
characters, including an unmatched quote.

## Commands

| Command | Aliases | Visible | Input | Defaults / notes |
|---|---|---|---|---|
| `/ping` | — | yes | no args | Runtime status only; no JVM/Kotlin/Spring fields |
| `/echo` | — | yes | raw optional text | Blank input returns usage |
| `/coin` | — | yes | optional `count: i64` | Default `1`; must be positive |
| `/dice` | — | yes | optional `count: i64` | Default `1`; must be positive |
| `/luck` | `l`, `jrrp` | yes | no args | Same user + local date is stable |
| `/24` | — | yes | raw optional | `<expression>`, `giveup`, or no input to start |
| `/start` | — | **no** | no args | Callable; hidden from help and help detail |
| `/help` | `h` | yes | optional `command`, optional `mode` | Text help only |
| `/weather` | — | yes | no args | Placeholder; no HTTP call |
| `/whois` | — | — | — | Not registered in this phase |

### `/ping`

```
Pong！<local date/time>
LukosBot <version>
运行时间：<d:hh:mm:ss>
系统：<os> <arch>
```

### `/coin` and `/dice`

Coin probabilities are `0.499999999999`, `0.499999999999`, and `0.000000000002`. Dice uses six
equal buckets. Both use the JVM sequential-binomial normal approximation; the last bucket receives
the remainder so count sums are exact. Dice totals use `u128`.

### `/luck`

The implementation mirrors the JVM `Objects.hashCode` / `LocalDate.hashCode` / `stableRandom`
path, including the SplitMix64 mixing constants. Numeric platform IDs use `Long.hashCode`;
non-numeric IDs fall back to Java `String.hashCode` and are documented as a small compatibility
extension.

Golden vectors generated from the JVM implementation:

| User ID | Date | Luck |
|---:|---|---:|
| 0 | 2026-10-23 | 30 |
| 42 | 2026-10-23 | 13 |
| 123456789 | 2026-10-23 | 98 |
| 987654321 | 2024-02-29 | 72 |
| 42 | 2026-01-01 | 10 |
| 0 | 2026-12-25 | 80 |

### `/24`

| Situation | Behaviour |
|---|---|
| No active game | Generate a solvable four-number puzzle (1..=13) |
| Active game | Report current numbers and how to continue |
| Expression is invalid | Explain allowed characters and operators |
| Result is not 24 | Show the result; game remains active |
| Result is 24 but numbers differ | Report the multiset mismatch; game remains active |
| Correct expression | Remove session and congratulate |
| `giveup` | Remove session and show a solution |
| Timeout | Remove session and send the solution to the original chat |

Session key: platform + user ID. Sessions are in-memory only. A single supervised worker manages
all deadlines; no detached timer tasks are created. Expression evaluation accepts only integers,
`+ - * /`, and parentheses, uses `f64` with `1e-6` tolerance, and rejects division by near-zero.

## Help

- `/help` lists visible commands in registration order.
- `/help <command>` resolves canonical names and aliases.
- `/help <command> text` renders text.
- `/help <command> img` returns an explicit “not implemented” message.
- Hidden commands are not listed and are treated as unknown in detail lookup (JVM behaviour).
- Help content is generated from command metadata; there is no hand-maintained command list.
- Policy filtering is not implemented in this phase.

## Known differences from the JVM bot

| Item | Difference |
|---|---|
| `/ping` | Rust-specific version, uptime, OS/architecture; no JVM/Kotlin/Spring/SDK versions |
| Duplicate commands | Startup failure instead of warning + bean-order-dependent selection |
| `/luck` non-numeric IDs | Java String hash fallback instead of failing to parse a numeric ID |
| `/start` copy | Product name is `LukosBot`; prefix is taken from configuration |
| `/help` | Text only; image rendering deferred |
| `/24` key | Platform + user ID instead of user ID alone |
| `/24` generation | Bounded attempts with a known-solvable fallback |
| `/weather` | Placeholder only |
| `/whois` | Deferred to the external-integration phase |
| Discord native slash commands | Deferred; this document covers text prefix commands only |
| Permissions | Deferred; all visible commands are currently available |

## Deferred commands and features

`/whois`, `/admin`, `/pref`, `/service`, the policy system, third-party APIs, media download and
upload, Wiki/McWiki, image help, screenshot rendering, Minecraft queries, H2 data import, and native
Discord interactions are intentionally outside this phase.
