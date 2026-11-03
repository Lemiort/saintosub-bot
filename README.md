# saintosub-bot

![Rust](https://github.com/Lemiort/saintosub-bot/workflows/Rust/badge.svg)

Telegram bot for the saintnosub group. Requires Rust 1.99.1, pinned in
`rust-toolchain.toml`, and uses the Rust 2024 edition.

Build and check:

```sh
cargo build --locked
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

Put your token in a `.env` file in the project directory, then run `cargo run --locked`:

```dotenv
TELEGRAM_BOT_TOKEN=your_bot_token
```

The bot automatically loads `.env` from the current directory or its parents.
Existing process environment variables take precedence. A missing `.env` is
allowed; an unreadable or malformed file produces a startup error. The `.env`
file is ignored by Git. Logs are written to `log/output.log`.
The bot must have permission to ban members for its 30-minute meme deadline.

### External LLM connection

The bot connects to an already-running OpenAI-compatible Chat Completions server,
such as LM Studio, Ollama's compatible endpoint, llama.cpp, or vLLM. It does not
launch, download, load, or manage models. Connection settings belong in the
existing ignored `.env`; `.env.example` documents the available settings.

For the LM Studio model tested here:

```dotenv
LLM_ENABLED=true
LLM_BASE_URL=http://127.0.0.1:1234/v1
LLM_MODEL=google/gemma-4-26b-a4b-qat
LLM_MEDIA=image
# Optional, if your server requires authentication:
LLM_API_KEY=
```

Restart the bot after changing configuration. Without `LLM_ENABLED=true`, the
existing bot behavior remains available. The base URL includes the provider's
API prefix, usually `/v1`; the bot appends `/chat/completions`.

When enabled, ordinary private messages go to the model. In groups, include an
explicit `@YourBotName` mention in the text or media caption; Telegram text
mentions linked to the bot's user ID also work. Unmentioned group messages are
not sent to the model. The existing `/start`, `/help`, `/tabletki`, and
`/jojomeme` commands take priority. Mentioning the bot in a reply can include
the quoted message's text or attachment. This does not automatically read the
rest of the group conversation.

| Input | Handling |
| --- | --- |
| Text, captions | Sent as text |
| UTF-8 text/code/JSON/CSV documents | Sent as text; capped at 20,000 characters |
| Photos, image documents, static WebP stickers | Enable `image`; embedded as base64 `image_url` parts |
| Voice notes and audio documents | Enable `audio`; sent as `input_audio` with the original codec, including Ogg for Telegram voice notes |
| Videos, video notes, animations, video stickers | Enable `video`; uses the provider-specific `video_url` extension |
| PDF and other binary documents | Enable `file`; uses `file` content parts with filename and base64 data |
| Contacts, locations, polls, dice | Included as structured text |
| Animated TGS stickers or other unsupported messages | A clear unsupported-input reply |

Only enable media types supported by **both the server API and the model**.
LM Studio reported vision support for the tested Gemma model; text and PNG
image input were tested successfully. Audio/video/file APIs and accepted codecs
vary by provider; this bot does not transcode attachments or extract video
frames. Unsupported types are rejected explicitly rather than silently omitted.
Media is downloaded from Telegram within the configured limit and embedded in
the request; Telegram token-bearing download URLs are never sent to the model.
Albums are handled as individual Telegram messages.

The bot keeps the last four exchanges per chat and sender in memory, with at
most 64 conversations. Group users have separate histories. Histories disappear
on restart or eviction; raw media is not retained between turns. Long responses
are split into Telegram-sized text messages.

Additional optional settings:

| Variable | Default | Purpose |
| --- | --- | --- |
| `LLM_MEDIA` | empty | Comma-separated `image,audio,video,file` capabilities |
| `LLM_TIMEOUT_SECS` | `180` | HTTP request timeout, 1–3600 seconds |
| `LLM_MAX_ATTACHMENT_MB` | `10` | Attachment size limit, 1–20 MiB |
| `LLM_MAX_TOKENS` | `2048` | Model output token limit, including any reasoning budget imposed by the server |
| `LLM_HISTORY_TURNS` | `4` | Retained exchanges per conversation, 1–20 |
| `LLM_SYSTEM_PROMPT` | Helpful, concise, user's language | Optional system instruction |

Check the model connection without connecting to Telegram or sending messages:

```sh
cargo run --locked -- --check-llm
cargo run --locked -- --check-llm --llm-image path/to/image.png
```

These checks send a synthetic prompt (and the specified image, if supplied) to
the configured provider. The bot cannot determine every provider's capabilities
automatically; use `LLM_MEDIA` to declare them.

### Telegram commands and diagnostics

Commands: `/start` and `/help` confirm the bot is online, `/tabletki` sends Giorno
drugs photo, and `/jojomeme` fetches an image from
r/ShitpostCrusaders. The bot greets new members and requires them to post a photo
within 30 minutes.

Startup and message handling logs appear in the terminal and `log/output.log`.
Commands addressed to the bot use its actual Telegram username, for example
`/jojomeme@YourBotName` in a group.

Check the `.env` token and Telegram configuration without polling updates or
sending messages:

```sh
cargo run --locked -- --check-telegram
```

If no messages arrive, stop the running bot and use
`cargo run --locked -- --check-updates` to inspect pending update types without
acknowledging them or printing message contents. Do not run this alongside a
polling instance. For detailed polling progress, start the bot with
`cargo run --locked -- --trace-polling`.

To test replies, run `cargo run --locked`, leave it running, and send `/start` in
a private chat with the username printed at startup. Only run one polling
instance per bot token. In groups, privacy mode can prevent ordinary text and
photos from reaching the bot; use commands addressed to its username. The meme
deadline needs the bot to receive photos, so grant it administrator permissions
or disable privacy mode through BotFather.

Meme fetching first tries Reddit's public hot feed. If Reddit rejects the request
or returns no supported images, it falls back to
[Meme API](https://github.com/D3vd/Meme_Api) for the same subreddit. Requests have
timeouts; source failures are logged and reported to the user without stopping
the bot. Only JPG, JPEG, and PNG image URLs are selected.

Verify fetching and downloading a real meme image without a Telegram token or
sending any messages:

```sh
cargo run --locked -- --check-memes
```

This live check requires internet access. Automated tests use local HTTP servers
and do not depend on Reddit or Meme API being available.

Known dependency limitation: teloxide pulls in `proc-macro-error2` 2.0.1 through
its documentation macros. Rust reports a future compatibility warning for that
crate, and RustSec flags it as unmaintained
([RUSTSEC-2026-0173](https://rustsec.org/advisories/RUSTSEC-2026-0173)). It builds
with the pinned toolchain; resolving this warning requires an upstream update.
