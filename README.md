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
