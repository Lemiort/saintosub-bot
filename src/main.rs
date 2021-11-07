mod memes;

use log::LevelFilter;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;

use std::{collections::HashSet, env, thread, time};

use tokio::sync::watch;
use tokio::sync::watch::Receiver;
use tokio::sync::watch::Sender;

use futures::executor::block_on;
use futures::StreamExt;
use telegram_bot::prelude::*;
use telegram_bot::{
    Api, Error, GetMe, InputFileRef, Message, MessageChat, MessageKind, UpdateKind, User,
};

const DRUGS_LINK: &str = "https://i.pinimg.com/474x/42/5e/53/425e5339585cf1435c76a0b4457693f8.jpg";

const SECS_TO_BAN: u64 = 1800; // 30 min
const POLLING_DELAY_SECS: u64 = 1800; // 30 min

pub struct SaintnosubBot {
    api: Api,
    memes_reader: memes::MemeReader,
    memeless_users: HashSet<User>,
    sender: Sender<User>,
    receiver: Receiver<User>,
}

impl SaintnosubBot {
    pub fn new(api: Api) -> Self {
        let future = api.send(GetMe);
        let me = block_on(future).unwrap();
        let (sender, receiver) = watch::channel(me);
        return SaintnosubBot {
            api: api,
            memes_reader: memes::MemeReader::new(),
            memeless_users: HashSet::new(),
            sender: sender,
            receiver: receiver,
        };
    }
    async fn reply_with_photo(&self, message: Message, link: String) -> Result<(), Error> {
        let mut photo =
            telegram_bot::requests::SendPhoto::new(message.chat, InputFileRef::new(link));
        photo.reply_to(message.id);
        self.api.send(photo).await?;
        Ok(())
    }

    async fn send_animation(&self, message: Message, link: String) -> Result<(), Error> {
        let file = InputFileRef::new(link);
        self.api.send(message.chat.video(file)).await?;
        Ok(())
    }

    async fn drugs_message(&self, message: Message) -> Result<(), Error> {
        self.reply_with_photo(message, String::from(DRUGS_LINK))
            .await?;
        Ok(())
    }

    async fn wait_for_meme(
        &mut self,
        chat: MessageChat,
        users: HashSet<User>,
    ) -> Result<(), Error> {
        for user in &users {
            log::info!("Waiting for meme from {}", user.first_name);
        }
        for user in users {
            let api = self.api.clone();
            let receiver = self.receiver.clone();
            let chat_copy = chat.clone();
            let _future = std::thread::spawn(move || {
                let start = time::Instant::now();
                let mut ban = true;
                let mut duration = start.elapsed();
                while duration < time::Duration::from_secs(SECS_TO_BAN) {
                    let latest_user = receiver.borrow();
                    if latest_user.id == user.id {
                        ban = false;
                        break;
                    }
                    duration = start.elapsed();
                    let polling_delay = time::Duration::from_millis(POLLING_DELAY_SECS);
                    thread::sleep(polling_delay);
                }
                if ban {
                    log::info!("Kicking {} for no meme", user.first_name);
                    let future = api.send(chat_copy.kick(user));
                    let _result = block_on(future).unwrap();
                }
            });
        }
        Ok(())
    }

    async fn greet_users(&mut self, message: Message) -> Result<(), Error> {
        if let MessageKind::NewChatMembers { ref data, .. } = message.kind {
            let me = self.api.send(GetMe).await?;
            let mut usernames = String::new();
            let mut users = HashSet::new();
            for user in data {
                if user.id != me.id {
                    users.insert(user.clone());
                    self.memeless_users.insert(user.clone());
                    if let Some(login) = &user.username {
                        usernames = format!("{} @{}, ", usernames, login);
                    } else {
                        usernames = format!("{} {}, ", usernames, user.first_name);
                    }
                }
            }
            let greeting = format!("{} мем или бан!", usernames);
            self.api.send(message.clone().text_reply(greeting)).await?;
            self.wait_for_meme(message.clone().chat, users).await?;
        }
        Ok(())
    }
    async fn goodbye_user(&self, message: Message) -> Result<(), Error> {
        if let MessageKind::LeftChatMember { ref data, .. } = message.kind {
            let result = self.api.send(GetMe).await?;
            if data.id != result.id {
                let greeting = String::from("Скатерью дорожка");
                self.api.send(message.text_reply(greeting)).await?;
            }
        }
        Ok(())
    }

    async fn send_meme(&mut self, message: Message) -> Result<(), Error> {
        let link = self.memes_reader.get_meme();
        self.reply_with_photo(message, link).await?;
        Ok(())
    }
    async fn parse_message(&self, message: Message) -> Result<(), Error> {
        if let MessageKind::Text { ref data, .. } = message.kind {
            let text = data.as_str().to_lowercase();
            if text.contains("дементий") {
                self.send_animation(message, memes::get_random_pig())
                    .await?;
            }
        }
        Ok(())
    }
    async fn parse_photo(&mut self, message: Message) -> Result<(), Error> {
        if let MessageKind::Photo { .. } = message.kind {
            if self.memeless_users.contains(&message.from) {
                log::info!("Removed {} fom possible bans", message.from.first_name);
                self.memeless_users.remove(&message.from);
                let _result = self.sender.send(message.from);
            }
        }
        Ok(())
    }

    async fn handle_message(&mut self, message: Message) -> Result<(), Error> {
        match message.kind {
            MessageKind::Text { ref data, .. } => match data.as_str() {
                "/tabletki" => self.drugs_message(message).await?,
                "/jojomeme" => self.send_meme(message).await?,
                "/jojomeme@saintnosubbot" => self.send_meme(message).await?,
                _ => self.parse_message(message).await?,
            },
            MessageKind::NewChatMembers { .. } => self.greet_users(message).await?,
            MessageKind::LeftChatMember { .. } => self.goodbye_user(message).await?,
            MessageKind::Photo { .. } => self.parse_photo(message).await?,
            _ => (),
        };
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{l} - {m}\n")))
        .build("log/output.log")
        .unwrap();

    let config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder().appender("logfile").build(LevelFilter::Info))
        .unwrap();

    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");

    log4rs::init_config(config).unwrap();
    log::info!("Starting saintnosub_bot...");

    let api = Api::new(token);
    let mut stream = api.stream();

    let mut bot = SaintnosubBot::new(api.clone());

    while let Some(update) = stream.next().await {
        let update = update?;
        if let UpdateKind::Message(message) = update.kind {
            bot.handle_message(message).await?;
        }
    }

    Ok(())
}
