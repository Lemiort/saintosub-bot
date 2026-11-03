mod llm;
mod memes;
#[cfg(test)]
mod test_http;

use std::{collections::HashSet, env, sync::Arc, time::Duration};

use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;
use teloxide::{
    prelude::*,
    types::{AllowedUpdate, InputFile, ReplyParameters, UpdateKind},
};
use tokio::sync::Mutex;

const DRUGS_LINK: &str = "https://i.pinimg.com/474x/42/5e/53/425e5339585cf1435c76a0b4457693f8.jpg";
const SECS_TO_BAN: u64 = 1800;
type PendingUsers = Arc<Mutex<HashSet<(ChatId, UserId)>>>;

struct SaintnosubBot {
    api: Bot,
    memes_reader: Mutex<memes::MemeReader>,
    memeless_users: PendingUsers,
    me: UserId,
    username: String,
    llm: Option<llm::LlmClient>,
}

impl SaintnosubBot {
    async fn reply_with_photo(&self, message: &Message, link: &str) -> ResponseResult<()> {
        let url = link.parse().expect("image source returned a validated URL");
        self.api
            .send_photo(message.chat.id, InputFile::url(url))
            .reply_parameters(ReplyParameters::new(message.id))
            .await?;
        Ok(())
    }

    async fn handle_message(&self, message: Message) -> ResponseResult<()> {
        log::info!(
            "Received message {} in chat {}",
            message.id,
            message.chat.id
        );
        let mut handled = false;
        if let Some(users) = message.new_chat_members() {
            handled = true;
            let mut usernames = Vec::new();
            for user in users.iter().filter(|user| user.id != self.me) {
                let key = (message.chat.id, user.id);
                self.memeless_users.lock().await.insert(key);
                usernames.push(
                    user.username
                        .as_ref()
                        .map_or_else(|| user.first_name.clone(), |name| format!("@{name}")),
                );
                let pending = Arc::clone(&self.memeless_users);
                let api = self.api.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(SECS_TO_BAN)).await;
                    if pending.lock().await.remove(&key) {
                        log::info!("Kicking {} for no meme in {}", key.1, key.0);
                        if let Err(error) = api.ban_chat_member(key.0, key.1).await {
                            log::error!("Unable to kick user: {error}");
                        }
                    }
                });
            }
            if !usernames.is_empty() {
                self.api
                    .send_message(
                        message.chat.id,
                        format!("{} мем или бан!", usernames.join(", ")),
                    )
                    .reply_parameters(ReplyParameters::new(message.id))
                    .await?;
            }
        } else if let Some(user) = message.left_chat_member() {
            handled = true;
            self.memeless_users
                .lock()
                .await
                .remove(&(message.chat.id, user.id));
            if user.id != self.me {
                self.api
                    .send_message(message.chat.id, "Скатертью дорожка")
                    .reply_parameters(ReplyParameters::new(message.id))
                    .await?;
            }
        } else if message.photo().is_some() {
            if let Some(user) = &message.from {
                self.memeless_users
                    .lock()
                    .await
                    .remove(&(message.chat.id, user.id));
            }
        } else if let Some(text) = message.text() {
            match command_name(text, &self.username) {
                Some("start" | "help") => {
                    handled = true;
                    self.api.send_message(message.chat.id,
                        "Bot is online. /jojomeme — fetch a JoJo meme; /tabletki — send the drugs photo. When LLM is enabled, chat privately or mention me in a group; media support depends on the configured model.")
                        .reply_parameters(ReplyParameters::new(message.id)).await?;
                }
                Some("tabletki") => {
                    handled = true;
                    self.reply_with_photo(&message, DRUGS_LINK).await?;
                }
                Some("jojomeme") => {
                    handled = true;
                    log::info!("Fetching meme for chat {}", message.chat.id);
                    let result = self.memes_reader.lock().await.get_meme().await;
                    match result {
                        Ok(link) => self.reply_with_photo(&message, &link).await?,
                        Err(error) => {
                            log::error!("Unable to fetch meme: {error}");
                            self.api
                                .send_message(
                                    message.chat.id,
                                    "Memes are unavailable right now. Please try again later.",
                                )
                                .reply_parameters(ReplyParameters::new(message.id))
                                .await?;
                        }
                    }
                }
                None if !text.trim_start().starts_with('/')
                    && (self.llm.is_none()
                        || !llm::should_respond(&message, self.me, &self.username))
                    && text.to_lowercase().contains("дементий") =>
                {
                    handled = true;
                    let url = memes::get_random_pig().parse().expect("invalid pig URL");
                    self.api
                        .send_video(message.chat.id, InputFile::url(url))
                        .await?;
                }
                _ => {}
            }
        }
        if !handled
            && llm::should_respond(&message, self.me, &self.username)
            && let Some(llm) = &self.llm
        {
            let reply = match llm.answer(&self.api, &message).await {
                Ok(reply) => reply,
                Err(error) => {
                    log::error!("LLM request failed: {error:#}");
                    error.downcast_ref::<llm::InputError>().map_or_else(
                        || "The language model is unavailable or rejected this input. Please try again later.".to_owned(),
                        |error| error.to_string())
                }
            };
            for chunk in llm::reply_chunks(&reply) {
                self.api
                    .send_message(message.chat.id, chunk)
                    .reply_parameters(ReplyParameters::new(message.id))
                    .await?;
            }
        }
        Ok(())
    }
}

fn command_name<'a>(text: &'a str, username: &str) -> Option<&'a str> {
    let command = text.split_whitespace().next()?.strip_prefix('/')?;
    match command.split_once('@') {
        Some((name, target)) if target.eq_ignore_ascii_case(username) => Some(name),
        Some(_) => None,
        None => Some(command),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load configuration before Tokio starts its worker threads.
    if let Err(error) = dotenvy::dotenv()
        && !error.not_found()
    {
        return Err(std::io::Error::other(
            "Unable to load .env; check its syntax and file permissions",
        )
        .into());
    }
    run()
}

#[tokio::main]
async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let llm = llm::Config::from_env()?
        .map(llm::LlmClient::new)
        .transpose()?;
    if env::args().any(|arg| arg == "--check-llm") {
        let llm = llm.as_ref().ok_or_else(|| {
            std::io::Error::other("Set LLM_ENABLED=true and configure LLM_MODEL in .env first")
        })?;
        let arguments: Vec<_> = env::args().collect();
        let image = arguments
            .windows(2)
            .find(|pair| pair[0] == "--llm-image")
            .map(|pair| std::path::Path::new(&pair[1]));
        println!("LLM response: {}", llm.check(image).await?);
        return Ok(());
    }
    if env::args().any(|arg| arg == "--check-memes") {
        let url = memes::MemeReader::new()?.check().await?;
        println!("Successfully fetched meme image: {url}");
        return Ok(());
    }

    std::fs::create_dir_all("log")?;
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{l} - {m}\n")))
        .build("log/output.log")?;
    let mut config = Config::builder();
    if env::args().any(|arg| arg == "--trace-polling") {
        config = config.logger(
            Logger::builder().build("teloxide::update_listeners::polling", LevelFilter::Trace),
        );
    }
    let config = config
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .appender(
            Appender::builder().build(
                "console",
                Box::new(
                    ConsoleAppender::builder()
                        .encoder(Box::new(PatternEncoder::new("{l} - {m}\n")))
                        .build(),
                ),
            ),
        )
        .build(
            Root::builder()
                .appender("logfile")
                .appender("console")
                .build(LevelFilter::Info),
        )?;
    log4rs::init_config(config)?;
    log::info!(
        "LLM integration: {}",
        if llm.is_some() { "enabled" } else { "disabled" }
    );

    let token = env::var("TELEGRAM_BOT_TOKEN")?;
    let api = Bot::new(token);
    let me = api.get_me().await?;
    log::info!("Authenticated as @{}", me.username());
    if env::args().any(|arg| arg == "--check-updates") {
        // No offset: inspecting pending updates must not acknowledge them.
        let updates = api
            .get_updates()
            .timeout(0)
            .limit(10)
            .allowed_updates(vec![AllowedUpdate::Message])
            .await?;
        println!("Pending updates returned: {}", updates.len());
        for update in updates {
            match update.kind {
                UpdateKind::Message(message) => println!(
                    "Update {}: message in {} chat (text: {}, photo: {})",
                    update.id.0,
                    if message.chat.is_private() {
                        "private"
                    } else {
                        "group"
                    },
                    message.text().is_some(),
                    message.photo().is_some()
                ),
                UpdateKind::Error(_) => {
                    println!("Update {}: unrecognized Telegram update", update.id.0)
                }
                _ => println!("Update {}: other Telegram update", update.id.0),
            }
        }
        return Ok(());
    }
    if env::args().any(|arg| arg == "--check-telegram") {
        let webhook = api.get_webhook_info().await?;
        println!("Telegram authentication: OK (@{})", me.username());
        println!("Webhook configured: {}", webhook.url.is_some());
        println!("Pending updates: {}", webhook.pending_update_count);
        println!(
            "Group privacy mode disabled: {}",
            me.can_read_all_group_messages
        );
        return Ok(());
    }
    log::info!(
        "Starting polling. Send /start or /jojomeme to @{}",
        me.username()
    );
    let bot = Arc::new(SaintnosubBot {
        api: api.clone(),
        memes_reader: Mutex::new(memes::MemeReader::new()?),
        memeless_users: Arc::new(Mutex::new(HashSet::new())),
        me: me.id,
        username: me.username().to_owned(),
        llm,
    });
    log::info!("Starting saintnosub_bot...");
    let check_dispatch = env::args().any(|arg| arg == "--check-dispatch");
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let dispatch = teloxide::repl(api, move |message: Message| {
        let bot = Arc::clone(&bot);
        let sender = sender.clone();
        async move {
            if check_dispatch {
                let _ = sender.send(message.id).await;
            } else if let Err(error) = bot.handle_message(message).await {
                log::error!("Telegram message handling failed: {error}");
            }
            respond(())
        }
    });
    if check_dispatch {
        tokio::select! {
            () = dispatch => return Err(std::io::Error::other("Dispatcher exited before receiving a message").into()),
            result = tokio::time::timeout(Duration::from_secs(20), receiver.recv()) => {
                let message = result?.ok_or_else(|| std::io::Error::other("Dispatcher stopped"))?;
                println!("Telegram polling and dispatch: OK (message {message}); no reply sent");
            }
        }
    } else {
        dispatch.await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn llm_test_bot(api_url: &str, provider_url: &str) -> SaintnosubBot {
        let settings = llm::Config::from_lookup(|key| match key {
            "LLM_BASE_URL" => Some(format!("{provider_url}/v1")),
            "LLM_MODEL" => Some("test-model".into()),
            "LLM_MEDIA" => Some("image".into()),
            _ => None,
        })
        .unwrap();
        SaintnosubBot {
            api: Bot::new("123:TEST").set_api_url(api_url.parse().unwrap()),
            memes_reader: Mutex::new(memes::MemeReader::new().unwrap()),
            memeless_users: Arc::new(Mutex::new(HashSet::new())),
            me: UserId(99),
            username: "ActualBot".into(),
            llm: Some(llm::LlmClient::new(settings).unwrap()),
        }
    }

    fn mock_telegram_reply() -> Vec<u8> {
        br#"{"ok":true,"result":{"message_id":2,"date":0,"chat":{"id":7,"type":"private"},"text":"ok"}}"#.to_vec()
    }

    #[tokio::test]
    async fn private_text_uses_llm_while_unmentioned_group_text_is_ignored() {
        use crate::test_http::MockServer;
        let telegram = MockServer::new(vec![(200, mock_telegram_reply())]);
        let provider = MockServer::new(vec![(
            200,
            br#"{"choices":[{"message":{"content":"LLM answer"}}]}"#.to_vec(),
        )]);
        let bot = llm_test_bot(&telegram.url, &provider.url);
        let private: Message = serde_json::from_value(serde_json::json!({
            "message_id":1, "date":0, "chat":{"id":7,"type":"private"},
            "from":{"id":7,"is_bot":false,"first_name":"Tester"}, "text":"hello model"
        }))
        .unwrap();
        bot.handle_message(private).await.unwrap();
        let group = serde_json::from_value(serde_json::json!({
            "message_id":2, "date":0, "chat":{"id":-7,"type":"supergroup","title":"Test"},
            "from":{"id":7,"is_bot":false,"first_name":"Tester"}, "text":"ordinary group text"
        }))
        .unwrap();
        bot.handle_message(group).await.unwrap();
        assert_eq!(provider.requests().len(), 1);
        let reply: serde_json::Value =
            serde_json::from_slice(&telegram.requests()[0].body).unwrap();
        assert_eq!(reply["text"], "LLM answer");
    }

    #[tokio::test]
    async fn mentioned_group_photo_reaches_llm_and_still_clears_meme_deadline() {
        use crate::test_http::MockServer;
        let telegram = MockServer::new(vec![
            (200, br#"{"ok":true,"result":{"file_id":"photo","file_unique_id":"unique","file_size":3,"file_path":"photos/test.jpg"}}"#.to_vec()),
            (200, vec![1,2,3]), (200, mock_telegram_reply())]);
        let provider = MockServer::new(vec![(
            200,
            br#"{"choices":[{"message":{"content":"I see the image"}}]}"#.to_vec(),
        )]);
        let bot = llm_test_bot(&telegram.url, &provider.url);
        bot.memeless_users
            .lock()
            .await
            .insert((ChatId(-7), UserId(7)));
        let photo = serde_json::from_value(serde_json::json!({
            "message_id":1,"date":0,"chat":{"id":-7,"type":"supergroup","title":"Test"},
            "from":{"id":7,"is_bot":false,"first_name":"Tester"},
            "caption":"@ActualBot describe","caption_entities":[{"type":"mention","offset":0,"length":10}],
            "photo":[{"file_id":"photo","file_unique_id":"unique","file_size":3,"width":1,"height":1}]
        })).unwrap();
        bot.handle_message(photo).await.unwrap();
        assert!(bot.memeless_users.lock().await.is_empty());
        assert_eq!(provider.requests().len(), 1);
        let requests = telegram.requests();
        let reply: serde_json::Value = serde_json::from_slice(&requests[2].body).unwrap();
        assert_eq!(reply["chat_id"], -7);
        assert_eq!(reply["text"], "I see the image");
    }

    #[tokio::test]
    async fn repl_polls_and_dispatches_a_private_command() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let api = Bot::new("123:TEST").set_api_url(
            format!("http://{}/", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
        );
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            let mut sent_update = false;
            while !server_stop.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                let (mut socket, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("mock Telegram accept failed: {error}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = BufReader::new(&mut socket);
                let mut first_line = String::new();
                request.read_line(&mut first_line).unwrap();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert!(request.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
                request.read_exact(&mut vec![0; length]).unwrap();
                let method = first_line.to_ascii_lowercase();
                let body = if method.contains("/getme ") {
                    r#"{"ok":true,"result":{"id":99,"is_bot":true,"first_name":"Test","username":"ActualBot","can_join_groups":true,"can_read_all_group_messages":false,"supports_inline_queries":false,"has_main_web_app":false}}"#
                } else if method.contains("/getwebhookinfo ") {
                    r#"{"ok":true,"result":{"url":"","has_custom_certificate":false,"pending_update_count":1}}"#
                } else if method.contains("/getupdates ") && !sent_update {
                    sent_update = true;
                    r#"{"ok":true,"result":[{"update_id":42,"message":{"message_id":1,"date":0,"chat":{"id":7,"type":"private"},"from":{"id":7,"is_bot":false,"first_name":"Tester"},"text":"/start"}}]}"#
                } else {
                    r#"{"ok":true,"result":[]}"#
                };
                let _ = write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let dispatcher = tokio::spawn(teloxide::repl(api, move |message: Message| {
            let sender = sender.clone();
            async move {
                let _ = sender.send(message).await;
                respond(())
            }
        }));
        let received = tokio::time::timeout(Duration::from_secs(10), receiver.recv()).await;
        dispatcher.abort();
        let _ = dispatcher.await;
        stop.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert_eq!(received.unwrap().unwrap().text(), Some("/start"));
    }

    async fn request_for_command(text: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = BufReader::new(&mut socket);
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(request.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut body = vec![0; length];
            request.read_exact(&mut body).unwrap();
            let response = r#"{"ok":true,"result":{"message_id":2,"date":0,"chat":{"id":7,"type":"private"},"text":"ok"}}"#;
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
            String::from_utf8(body).unwrap()
        });
        let bot = SaintnosubBot {
            api: Bot::new("123:TEST").set_api_url(url.parse().unwrap()),
            memes_reader: Mutex::new(memes::MemeReader::new().unwrap()),
            memeless_users: Arc::new(Mutex::new(HashSet::new())),
            me: UserId(99),
            username: "ActualBot".into(),
            llm: None,
        };
        let message = serde_json::from_value(serde_json::json!({
            "message_id": 1, "date": 0,
            "chat": {"id": 7, "type": "private"}, "text": text,
        }))
        .unwrap();
        tokio::time::timeout(Duration::from_secs(10), bot.handle_message(message))
            .await
            .unwrap()
            .unwrap();
        server.join().unwrap()
    }

    #[tokio::test]
    async fn start_sends_a_telegram_reply() {
        let request: serde_json::Value =
            serde_json::from_str(&request_for_command("/start").await).unwrap();
        assert_eq!(request["chat_id"], 7);
        assert!(request["text"].as_str().unwrap().contains("Bot is online"));
        assert_eq!(request["reply_parameters"]["message_id"], 1);
    }

    #[tokio::test]
    async fn addressed_tabletki_sends_a_photo() {
        let request = request_for_command("/tabletki@ActualBot").await;
        assert!(request.contains("name=\"chat_id\"\r\n\r\n7\r\n"));
        assert!(request.contains(&format!("name=\"photo\"\r\n\r\n{DRUGS_LINK}\r\n")));
        assert!(request.contains("name=\"reply_parameters\"\r\n\r\n{\"message_id\":1}"));
    }

    #[test]
    fn commands_target_the_authenticated_bot() {
        assert_eq!(
            command_name("/jojomeme@ActualBot", "ActualBot"),
            Some("jojomeme")
        );
        assert_eq!(
            command_name("/tabletki@actualbot", "ActualBot"),
            Some("tabletki")
        );
        assert_eq!(command_name("/jojomeme@OtherBot", "ActualBot"), None);
    }

    #[test]
    fn commands_support_start_payloads_and_whitespace() {
        assert_eq!(command_name("/start referral", "ActualBot"), Some("start"));
        assert_eq!(command_name(" /jojomeme\n", "ActualBot"), Some("jojomeme"));
        assert_eq!(command_name("ordinary text", "ActualBot"), None);
    }
}
