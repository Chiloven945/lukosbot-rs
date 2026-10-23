# Third-party API behavior matrix

This document freezes the behavior implemented in the Rust third-party command phase. The JVM
`lukosBot2` sources are the behavioral reference; intentional deviations are listed explicitly.

## Commands

| Command               | Aliases | Input                                                         | Defaults                           | Successful output                                                             |
|-----------------------|---------|---------------------------------------------------------------|------------------------------------|-------------------------------------------------------------------------------|
| `/github` / `/gh`     | `gh`    | `user <username>`, `repo <owner/repo>`, `search <keyword...>` | `--top=3`; no default sort/order   | User profile, repository summary, or repository search list                   |
| `/bilibili` / `/bili` | `bili`  | `target` (BV/av/URL/B23 short link), `-i`                     | `-i` false                         | Simple or detailed video information; cover URL appended as text              |
| `/ip`                 | —       | `<ip>` plus `-p/--provider/--providers`                       | Default priority `ipquery`, `ipsb` | Normalized IP data and the successful provider; fallback list when applicable |
| `/music`              | —       | `[spotify\|s\|soundcloud\|sc] <query...>`, `link <url>`       | Spotify before SoundCloud          | Normalized track information                                                  |
| `/whois`              | —       | `<domain>` (raw)                                              | —                                  | Cleaned WHOIS text, up to 1,800 characters                                    |

Command toggles live in `[commands.integrations]` and default to `true`. Disabled commands are not
registered, so they disappear from normal help output and cannot be invoked. Policy continues to be
applied to the canonical root command before argument parsing.

## GitHub

- Base URL: `https://api.github.com`.
- `Accept: application/vnd.github.v3+json`; `Authorization: Bearer <token>` only when a non-empty
  token is configured.
- User: `GET /users/{username}`.
- Repository: `GET /repos/{owner}/{repo}`.
- Search: `GET /search/repositories` with `q`, `per_page`, and optional `sort`/`order`.
- `--lang` is merged into the query as `language:<lang>`.
- `--top` is validated to 1–10 before the request and the response is truncated to it.
- `--sort` accepts `stars` or `updated`; `--order` accepts `asc` or `desc`. When omitted, neither
  query parameter is sent (matching the JVM command, whose options have no defaults).
- Errors are categorized as not found, unauthorized, rate limited, timeout, transport, invalid
  response, or upstream HTTP status. Provider response bodies are never echoed to users.

## Bilibili

- Target parsing is pure and happens before any network request.
- Accepted ids: `BV` followed by 10 ASCII alphanumerics; `av` followed by a positive decimal. Prefix
  matching is case-insensitive; the normalized BV form is upper-case.
- URLs must use `http` or `https` and an exact host from: `bilibili.com`, `www.bilibili.com`,
  `m.bilibili.com`, `b23.tv`, `www.b23.tv`.
- B23 short links are followed manually with a no-redirect HTTP client. Every redirect target is
  re-parsed and checked against the allowlist; at most five redirects are allowed.
- Video endpoint: `GET https://api.bilibili.com/x/web-interface/view` with `bvid` or `aid`.
- A nonzero envelope `code` or missing `data` is treated as not found.
- `GET https://api.bilibili.com/x/relation/stat` is best-effort. Its failure leaves followers
  unknown and never discards the video result.
- Simple output: link, title, category, uploader, date, and the `-i` hint.
- Detailed output: link, title/page count, category, uploader and followers, description (160-char
  limit), view/danmaku/reply/like/coin/favorite/share counts, and date. The cover URL is appended as
  text.
- Times come from Unix seconds and are rendered in UTC; malformed timestamps are shown as `-`.
- The low-level timeout budgets are 12s total, 8s for the view request, 4s for relation statistics,
  and the remaining deadline for short-link resolution.

Intentional tightening: the JVM Bilibili code accepts arbitrary `http` URLs and follows redirects
with its normal client. The Rust implementation rejects non-Bilibili hosts and validates every hop.

## IP

| Canonical id | Aliases                  | Priority | Endpoint                       |
|--------------|--------------------------|---------:|--------------------------------|
| `ipquery`    | `ipquery.io`, `ip-query` |      100 | `https://api.ipquery.io/{ip}`  |
| `ipsb`       | `ip.sb`, `ip-sb`         |       80 | `https://api.ip.sb/geoip/{ip}` |

- The command parses `std::net::IpAddr` before any request; hostnames, CIDR, and malformed values
  are rejected. This is stricter than the JVM implementation.
- With no `--provider`, providers are attempted in priority order. With an explicit list, the user
  order is preserved and duplicates are removed.
- Unknown provider names fail before network I/O and list the available providers.
- A failed provider is recorded and the next one is attempted. The successful result reports the
  provider that succeeded and the providers that failed.
- All providers failing produces an aggregate error containing every attempted provider id. Raw
  provider errors are logged by category, not returned to chat.
- Each provider has a 3s budget and the whole fallback chain has a 6s budget.

## Music

- `available_platforms()` returns configured providers in order: Spotify, then SoundCloud.
- Search without a platform prefix uses Spotify when available, otherwise SoundCloud.
- Explicit `spotify`/`s` and `soundcloud`/`sc` never fall back to the other provider.
- No configured provider returns `音乐平台暂未配置或不可用。`; an empty result returns
  `没有找到匹配的歌曲。`
- Spotify uses the Client Credentials flow, HTTP Basic authentication, and an in-memory token cache
  protected by `tokio::sync::Mutex`. Tokens expire with a 60s safety margin, are never persisted,
  and are refreshed at most once after a 401.
- Spotify search uses `limit=1`; links must be `https://open.spotify.com/track/{id}`.
- SoundCloud uses the API v2 search and resolve endpoints. `client_id` appears only in query
  parameters and is never logged. Accepted hosts are `soundcloud.com`, `www.soundcloud.com`,
  `m.soundcloud.com`, and `on.soundcloud.com`.
- Track fields are emitted in this order, omitting empty optional fields: platform, title, artist,
  album, duration (`mm:ss`), link, cover.
- Spotify ids are constructed as `https://open.spotify.com/track/{id}` when the API omits an
  external URL.

Intentional tightening: JVM code detects providers with string containment and accepts arbitrary
`http` links. Rust parses URLs, requires `https`, rejects credentials/ports, and matches exact
hosts.

## WHOIS

- Domain normalization strips surrounding whitespace and at most one trailing dot, rejects any
  control character (including CR/LF), IP literals, ports, paths, and single-label names, then
  applies IDNA. At least two labels are required.
- `com`, `net`, and `edu` use `whois.internic.net` (the confirmed Commons Net 3.13.0
  `WhoisClient.DEFAULT_HOST`).
- `cn` and IDNA `xn--fiqs8s` (`中国`) use `whois.cnnic.cn`.
- Other TLDs query `whois.iana.org` for a `whois:` referral. A successful IANA response without a
  valid referral returns not-found for that TLD; an IANA transport failure falls back to
  `whois.internic.net`.
- TCP port 43 is accessed with Tokio. DNS results must contain a public address; loopback, private,
  link-local, unspecified, multicast, documentation, broadcast, and IPv4-mapped private/loopback
  addresses are rejected. All public addresses are tried, IPv4 first, so a broken IPv6 route does
  not hide a working IPv4 registry server.
- Connection timeout, total deadline, and raw response size are configurable. The default raw limit
  is 64 KiB.
- Registry responses starting with `No match`, `Not found`, `Domain not found`, or
  `No entries found` are mapped to a not-found result instead of being displayed as a record.
- TLDs that publish no WHOIS referral (for example some modern Google TLDs) return not-found; RDAP
  fallback is outside the current `/whois` scope.
- Cleanup truncates at known boilerplate markers, normalizes line endings, removes empty key lines,
  merges duplicate keys in first-seen order, compresses blank runs, strips control characters, and
  limits the final text to 1,800 Unicode scalar values.
- A truncated result gets a clear truncation marker in the reply.

Intentional tightening: the JVM implementation uses blocking sockets with no timeouts and no DNS
address filtering.

## Output limits

Every integration reply is capped at 1,800 Unicode scalar values before being handed to the outbound
port. This keeps both Telegram and Discord within their text limits. Bilibili descriptions use a
160-character sub-limit. The outbound message model is unchanged.

## Credentials and configuration

```toml
[commands.integrations]
github = true
bilibili = true
ip = true
music = true
whois = true

[integrations.github]
token = ""

[integrations.music.spotify]
enabled = false
client_id = ""
client_secret = ""

[integrations.music.soundcloud]
enabled = false
client_id = ""

[integrations.whois]
connect_timeout_secs = 3
total_timeout_secs = 8
max_response_bytes = 65536
```

Environment variables use the existing `LUKOSBOT__...` prefix. `GITHUB_TOKEN` is a fallback only
when `LUKOSBOT__INTEGRATIONS__GITHUB__TOKEN` is absent. Enabled Spotify/SoundCloud providers with
incomplete credentials are logged once and marked unavailable; this does not prevent startup.
Secrets are redacted from `Debug`, configuration summaries, logs, errors, and user replies.

## Explicitly out of scope

- `/weather` remains a placeholder.
- Minecraft, wiki/web rendering, media download/grid, service scheduling, and Cave media are not
  part of this phase.
- The dispatcher remains serial; integration commands use bounded request budgets and do not spawn
  detached tasks.
