mod memes;

use std::{collections::HashSet, env, time};

use tokio::sync::watch;
use tokio::sync::watch::Receiver;
use tokio::sync::watch::Sender;

use teloxide::{
    prelude::*,
    types::{Chat, InputFile, MediaKind, MessageKind, User},
};

// const FOOL_PIC_LINK: &str = "https://c7.hotpng.com/preview/259/820/839/dio-brando-jojo-s-bizarre-adventure-eyes-of-heaven-youtube-diamond-is-unbreakable-just-cause-thumbnail.jpg";
// const APPROACHING_PIC_LINK: &str = "https://i.ytimg.com/vi/IJJM_ccGxSQ/maxresdefault.jpg";
const WTF_PIC_LINK: &str = "https://i.redd.it/dv7afptdh9131.jpg";
const ROAD_ROLLER_PIC_LINK: &str = "https://i.ytimg.com/vi/t1y3QOIRsYs/maxresdefault.jpg";
const DOESNT_MATTER_PIC_LINK: &str =
    "https://i.kym-cdn.com/entries/icons/original/000/029/407/Screenshot_14.jpg";

const SECS_TO_BAN: u64 = 1800; // 30 min

pub struct SaintnosubBot {
    bot: Bot,
    memeless_users: HashSet<User>,
    sender: Sender<User>,
    receiver: Receiver<User>,
    meme_reader: memes::MemeReader,
}

impl SaintnosubBot {
    pub fn new(bot: Bot) -> Self {
        let (sender, receiver) = watch::channel(User::new(0, true, "Foo"));

        return SaintnosubBot {
            bot: bot,
            memeless_users: HashSet::new(),
            sender: sender,
            receiver: receiver,
            meme_reader: memes::MemeReader::new(),
        };
    }
    async fn reply_with_photo(&self, message: Message, link: String) -> ResponseResult<()> {
        let photo = InputFile::Url(link);
        self.bot.send_photo(message.chat.id, photo);
        Ok(())
    }

    async fn send_animation(&self, message: Message, link: String) -> ResponseResult<()> {
        let file = InputFile::Url(link);
        self.bot.send_animation(message.chat.id, file);
        Ok(())
    }

    async fn drugs_message(&self, message: Message) -> ResponseResult<()> {
        // if let Some(reply_box) = message.reply_to_message {
        //     let value = *reply_box;
        //     if let MessageOrChannelPost::Message(org_message) = value {
        //         self.reply_with_photo(org_message, String::from(ROAD_ROLLER_PIC_LINK))
        //             .await?;
        //     }
        // } else {
        //     self.reply_with_photo(message, String::from(DOESNT_MATTER_PIC_LINK))
        //         .await?;
        // }
        let photo = InputFile::Url(String::from(ROAD_ROLLER_PIC_LINK));
        self.bot.send_photo(message.chat.id, photo);
        Ok(())
    }

    async fn wait_for_meme(&mut self, chat: Chat, users: HashSet<User>) -> ResponseResult<()> {
        for user in &users {
            println!("Wait for meme {} ", user.first_name);
        }
        for user in users {
            let bot = self.bot.clone();
            let receiver = self.receiver.clone();
            let chat_id = chat.id;
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
                }
                if ban {
                    // 15 mins to send a meme
                    println!("Kick {} ", user.first_name);
                    bot.kick_chat_member(chat_id, user.id);
                }
            });
        }
        Ok(())
    }

    async fn greet_users(&mut self, message: Message) -> ResponseResult<()> {
        let chat_clone = message.chat.clone();
        match message.kind {
            MessageKind::NewChatMembers(new_chat_members_struct) => {
                let mut usernames = String::new();
                let mut users = HashSet::new();
                for user in new_chat_members_struct.new_chat_members {
                    if !user.is_bot {
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
                self.bot.send_message(message.chat.id, greeting);
                self.wait_for_meme(chat_clone, users).await?;
            }
            _ => (),
        }

        Ok(())
    }
    async fn goodbye_user(&self, message: Message) -> ResponseResult<()> {
        if let MessageKind::LeftChatMember(..) = message.kind {
            let greeting = String::from("Скатерью дорожка");
            self.bot.send_message(message.chat.id, greeting);
        }
        Ok(())
    }

    async fn send_meme(&mut self, message: Message) -> ResponseResult<()> {
        let link = self.meme_reader.get_meme();
        self.reply_with_photo(message, link).await?;
        Ok(())
    }
    async fn parse_message(&self, message: Message) -> ResponseResult<()> {
        let message_clone = message.clone();
        match message.kind {
            MessageKind::Common(common_message) => match common_message.media_kind {
                MediaKind::Text(media_text) => {
                    let text = media_text.text.to_lowercase();
                    if text.contains("дементий") {
                        self.send_animation(message_clone, memes::get_random_pig())
                            .await?;
                    }
                }
                _ => (),
            },
            _ => (),
        }
        Ok(())
    }
    async fn parse_photo(&mut self, message: Message) -> ResponseResult<()> {
        let message_clone = message.clone();
        match message.kind {
            MessageKind::Common(common_message) => match common_message.media_kind {
                MediaKind::Photo(..) => {
                    if self.memeless_users.contains(&message_clone.from().unwrap()) {
                        log::info!(
                            "Removed {} fom possible bans",
                            message_clone.from().unwrap().first_name
                        );
                        self.memeless_users.remove(&message_clone.from().unwrap());
                        let _result = self.sender.broadcast(message_clone.from().unwrap().clone());
                    }
                }
                _ => (),
            },
            _ => (),
        }
        Ok(())
    }

    async fn handle_message(&mut self, message: Message) -> ResponseResult<()> {
        let message_clone = message.clone();
        match message.kind {
            MessageKind::Common(common_message) => match common_message.media_kind {
                MediaKind::Text(media_text) => match media_text.text.as_str() {
                    "/tabletki" => self.drugs_message(message_clone).await?,
                    "/jojomeme" => self.send_meme(message_clone).await?,
                    "/jojomeme@saintnosubbot" => self.send_meme(message_clone).await?,
                    _ => self.parse_message(message_clone).await?,
                },
                MediaKind::Photo(..) => self.parse_photo(message_clone).await?,
                _ => (),
            },
            MessageKind::NewChatMembers(..) => self.greet_users(message_clone).await?,
            MessageKind::LeftChatMember(..) => self.goodbye_user(message_clone).await?,
            _ => (),
        }
        Ok(())
    }

    // async fn run(&mut self) -> ResponseResult<()> {
    //     teloxide::repl(self.bot, |message| async move {
    //         return self.handle_message(message.update).await;
    //     })
    //     .await;
    // }
}

// Kick a user with a replied message.
async fn kick_user(cx: &UpdateWithCx<Message>) -> ResponseResult<()> {
    match cx.update.reply_to_message() {
        Some(mes) => {
            // bot.unban_chat_member can also kicks a user from a group chat.
            cx.bot
                .unban_chat_member(cx.update.chat_id(), mes.from().unwrap().id)
                .send()
                .await?;
        }
        None => {
            cx.reply_to("Use this command in reply to another message")
                .send()
                .await?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    teloxide::enable_logging!();
    log::info!("Starting saintnosub_bot...");

    let bot = Bot::from_env();

    let mut saintnosub_bot = SaintnosubBot::new(bot.clone());

    let bot_box = Box::new(&mut saintnosub_bot);

    let handler = |message: UpdateWithCx<Message>| async {
        let unboxed_bot = *bot_box;
        *bot_box.handle_message(message.update).await?;
        return ResponseResult::<()>::Ok(());
    };

    teloxide::repl(bot, handler).await;
}
