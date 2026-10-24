# Media and web behavior matrix

Status: implemented skeleton, automated tests cover the pure model, policy, parsing, rendering,
media loading, platform mapping, and runtime paths. Real platform/WebDriver acceptance remains
manual.

## Scope

| Area            | Commands / capability                                  | Status              |
|-----------------|--------------------------------------------------------|---------------------|
| Media transport | inbound image/file, outbound image/file, media loading | implemented         |
| Cave            | `/cave`, `/c`, `add`, `delete`                         | implemented         |
| E621            | `get artist/post`, `search artist/post/md5`, grid      | implemented         |
| Wikipedia       | `/wiki`, `/wiki md`                                    | implemented         |
| Minecraft Wiki  | `/mcwiki`, `/mcwiki md`, `/mcwiki ss`                  | implemented         |
| Image help      | `/help <command> img`, automatic image mode            | implemented         |
| Kemono          | all commands/API/config                                | explicitly excluded |

## Media

- `MediaSource::Bytes` means content is already materialized; URL and platform file sources must be
  resolved by `MediaLoader`.
- Outbound messages support one media part per message. Multiple media parts are rejected.
- Telegram uses `sendPhoto` and `sendDocument`; Discord uses `CreateAttachment`.
- Captions are combined with text parts. Platform caption/content limits are enforced explicitly,
  not silently truncated.
- Telegram platform files use `getFile` and a bounded adapter download. Discord attachments use the
  HTTP media loader.
- URL loading is HTTPS-first, host-allowlisted, redirect-validated, size-limited, and rejects
  private/loopback/link-local/mapped addresses.
- Image decoding, scaling, and encoding are bounded by byte and pixel limits.

## Cave

- `/cave` and `/c` randomly replay an entry.
- `/cave <number>` replays that entry.
- `/cave add` stores the quoted message when it has supported content; otherwise it stores the
  current message. Only the first image is stored.
- `/cave delete <number>` requires a bot administrator.
- Text and image payloads are stored in the compatibility namespaces `cmd.cave.meta` and
  `cmd.cave.entry`; image bytes are Base64 JSON for JVM compatibility.
- Metadata is repaired from the entry namespace when stale; corrupt entries are skipped.
- Image entries replay as image bytes with a metadata/text caption; text entries replay as text.

## E621

- `/e621 get artist <id-or-name>`.
- `/e621 get post <id-or-link>`; links must be `e621.net` article URLs.
- `/e621 search artist <text> [page]`; the trailing positive integer is the page.
- `/e621 search post <text> [page]`; same page rule.
- `/e621 search md5 <32 hex>`.
- Post requests use `v2=true` and `mode=extended`; legacy `posts`/`post` envelopes remain accepted
  as a fallback.
- Artist detail maps the current `Artist` schema: `other_names`, `notes`, `group_name`, `urls`,
  activity, timestamps, linked/creator IDs and lock state.
- Requests are anonymous. No Basic auth or API key is sent; the public read endpoints used by these
  commands do not require an account.
- All requests send a non-empty descriptive `User-Agent`; browser impersonation is not allowed.
- Page is limited to 1–750; the command page is 12 by default.
- E621 limit is at most 320; the command uses 12 by default. v2 extended `stats.score` is an
  object; the DTO also accepts the legacy integer shape.
- Requests are paced at one per second. 429/503 are treated as rate limiting, use `Retry-After`
  when present, and retry at most once. 404 is not-found; 5xx is upstream failure.
- Rating policy key is `e621.rating`, default `s/q/e`. Unknown ratings are not allowed.
- When explicit is disabled, the remote query gets `-rating:e` and results are filtered again before
  formatting and before any thumbnail download.
- Grid rendering uses only filtered posts. A grid failure falls back to the text result.
- Thumbnail priority is `files.preview.jpg`, `files.preview.webp`, `files.sample.*`, then
  `files.original.url`.

## Wiki

- Language prefixes use the JVM-compatible 1–6 character `[A-Za-z-]{2,6}` rule; default language is
  `zh`.
- Wikipedia uses `/wiki/`; Minecraft Wiki uses `/w/`.
- Only HTTPS article URLs on the matching wiki host are accepted. Lookalike domains, ports,
  credentials, IP literals, and `www` non-language hosts are rejected.
- Redirects are followed manually and revalidated on every hop.
- Minecraft summary uses `h1#firstHeading` and prefers `#mw-content-text .mw-parser-output`, falling
  back to `#content`; it reads the first two non-empty direct child paragraphs (up to 500 chars).
- Markdown export uses `scraper` plus `htmd`; filenames are sanitized and output is size-limited.
- Screenshots use a supervised worker, the original `wikiIntroMeasure.js` two-pass viewport
  measurement, and a configured WebDriver endpoint. Browser sessions are
  explicitly quit. Browser failure is per-job and does not stop the runtime worker.
- Screenshot output is JPEG with a sanitized title filename; dimensions and total pixels are
  bounded.
- No arbitrary URL screenshot command is registered.

## Image help

- Explicit `img`/`image`/`pic`/`png`/`图片`/`图` forces image rendering.
- Explicit `text`/`txt`/`raw`/`文字` forces text.
- Automatic mode uses image when the text help exceeds 1,400 characters or 32 lines.
- Image rendering failure falls back to text and adds a short notice.

## Deliberate differences from JVM

- Strict HTTPS and host allowlists for wiki/media URLs.
- Strict IP parsing and private-address rejection for media downloads.
- Explicit browser session cleanup and bounded screenshot queue.
- Platform caption/content limits fail explicitly instead of silently truncating.
- E621 search page parsing uses a raw trailing-number rule because the Rust command engine keeps
  greedy positional arguments last.
- `help img` uses structured usage metadata instead of parsing rendered Markdown.
