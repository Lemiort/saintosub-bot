extern crate rawr;
use std::{collections::HashSet, env, time};

use tokio::sync::watch;
use tokio::sync::watch::Receiver;
use tokio::sync::watch::Sender;

use futures::executor::block_on;
use futures::StreamExt;
use rand::seq::SliceRandom;
use telegram_bot::prelude::*;
use telegram_bot::{
    Api, Error, GetMe, InputFileRef, Message, MessageChat, MessageKind, MessageOrChannelPost,
    UpdateKind, User,
};

use rawr::prelude::*;

// const FOOL_PIC_LINK: &str = "https://c7.hotpng.com/preview/259/820/839/dio-brando-jojo-s-bizarre-adventure-eyes-of-heaven-youtube-diamond-is-unbreakable-just-cause-thumbnail.jpg";
// const APPROACHING_PIC_LINK: &str = "https://i.ytimg.com/vi/IJJM_ccGxSQ/maxresdefault.jpg";
const WTF_PIC_LINK: &str = "https://i.redd.it/dv7afptdh9131.jpg";
const ROAD_ROLLER_PIC_LINK: &str = "https://i.ytimg.com/vi/t1y3QOIRsYs/maxresdefault.jpg";
const DOESNT_MATTER_PIC_LINK: &str =
    "https://i.kym-cdn.com/entries/icons/original/000/029/407/Screenshot_14.jpg";

const PIGS_LINKS: &'static [&'static str] = &[
    "https://cs10.pikabu.ru/post_img/2019/06/14/8/1560517294111013742.gif",
    "https://cs11.pikabu.ru/post_img/2019/06/14/8/1560517238115787100.gif",
    "https://cs7.pikabu.ru/post_img/2019/06/14/8/156051726414098019.gif",
    "https://cs7.pikabu.ru/post_img/2019/06/14/8/15605172431238525.gif",
    "https://cs10.pikabu.ru/post_img/2019/06/14/8/1560517177190448543.gif",
    "https://cs13.pikabu.ru/post_img/2019/06/14/8/1560517198188894105.gif",
    "https://cs11.pikabu.ru/post_img/2019/06/14/8/1560517203152341690.gif",
    "https://cs10.pikabu.ru/post_img/2019/06/14/8/1560517207120834751.gif",
    "https://cs7.pikabu.ru/post_img/2019/06/14/8/1560517210125498145.gif",
    "https://cs11.pikabu.ru/post_img/2019/06/14/8/1560517218171725970.gif",
];

const SECS_TO_BAN: u64 = 1800; // 30 min

pub struct SaintnosubBot<'a> {
    api: Api,
    hot_listing: &'a mut rawr::structures::listing::Listing<'a>,
    memeless_users: HashSet<User>,
    sender: Sender<User>,
    receiver: Receiver<User>,
}

impl<'a> SaintnosubBot<'a> {
    pub fn new(api: Api, hot_listing: &'a mut rawr::structures::listing::Listing<'a>) -> Self {
        let future = api.send(GetMe);
        let me = block_on(future).unwrap();
        let (sender, receiver) = watch::channel(me);
        return SaintnosubBot {
            api: api,
            hot_listing: hot_listing,
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
        if let Some(reply_box) = message.reply_to_message {
            let value = *reply_box;
            if let MessageOrChannelPost::Message(org_message) = value {
                self.reply_with_photo(org_message, String::from(ROAD_ROLLER_PIC_LINK))
                    .await?;
            }
        } else {
            self.reply_with_photo(message, String::from(DOESNT_MATTER_PIC_LINK))
                .await?;
        }
        Ok(())
    }

    async fn wait_for_meme(
        &mut self,
        chat: MessageChat,
        users: HashSet<User>,
    ) -> Result<(), Error> {
        for user in &users {
            println!("Wait for meme {} ", user.first_name);
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
                    let lastest_user = receiver.borrow();
                    if lastest_user.id == user.id {
                        ban = false;
                        break;
                    }
                    duration = start.elapsed();
                }
                if ban {
                    // 15 mins to send a meme
                    println!("Kick {} ", user.first_name);
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
        let reply = self.get_last_meme();
        let mut text = String::from(WTF_PIC_LINK);
        if let Some(title) = reply {
            text = title;
        }
        self.reply_with_photo(message, text).await?;
        Ok(())
    }
    async fn parse_message(&self, message: Message) -> Result<(), Error> {
        if let MessageKind::Text { ref data, .. } = message.kind {
            let text = data.as_str().to_lowercase();
            if text.contains("дементий") {
                let random_pig_link = *PIGS_LINKS.choose(&mut rand::thread_rng()).unwrap();
                self.send_animation(message, String::from(random_pig_link.clone()))
                    .await?;
            }
        }
        Ok(())
    }
    async fn parse_photo(&mut self, message: Message) -> Result<(), Error> {
        if let MessageKind::Photo { .. } = message.kind {
            println!("There is photo from {}", message.from.first_name);
            if !message.from.is_bot {
                if self.memeless_users.contains(&message.from) {
                    println!("Removed {} fom possible bans", message.from.first_name);
                    self.memeless_users.remove(&message.from);
                    let _result = self.sender.broadcast(message.from);
                }
            }
        }
        Ok(())
    }

    async fn handle_message(&mut self, message: Message) -> Result<(), Error> {
        match message.kind {
            MessageKind::Text { ref data, .. } => match data.as_str() {
                // "/start" => start_message(api, message).await?,
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
    fn get_last_meme(&mut self) -> Option<String> {
        if let Some(post) = self.hot_listing.next() {
            return post.link_url();
        }
        return None;
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");

    let api = Api::new(token);
    let mut stream = api.stream();

    let client = RedditClient::new(
        "linux:saintnosubbot:v0.0.1 (by /u/Lemiort)",
        AnonymousAuthenticator::new(),
    );
    // Access the subreddit /r/ShitPostCrusaders.
    let subreddit = client.subreddit("ShitPostCrusaders");

    // Gets the hot listing of /r/ShitPostCrusaders. If the API request fails, we will panic with `expect`.
    let mut hot_listing = subreddit
        .new(ListingOptions::default())
        .expect("Could not fetch post listing!");

    let mut bot = SaintnosubBot::new(api.clone(), &mut hot_listing);

    while let Some(update) = stream.next().await {
        let update = update?;
        if let UpdateKind::Message(message) = update.kind {
            bot.handle_message(message).await?;
        }
    }

    Ok(())
}
