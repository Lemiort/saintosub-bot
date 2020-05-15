extern crate rawr;
use std::{collections::HashSet, env, thread, time};

use futures::StreamExt;
use telegram_bot::prelude::*;
use telegram_bot::{
    Api, Error, GetMe, InputFileRef, Message, MessageChat, MessageKind, MessageOrChannelPost,
    UpdateKind, User,
};
// use tokio::time::delay_for;

use rawr::prelude::*;

const FOOL_PIC_LINK: &str = "https://c7.hotpng.com/preview/259/820/839/dio-brando-jojo-s-bizarre-adventure-eyes-of-heaven-youtube-diamond-is-unbreakable-just-cause-thumbnail.jpg";
// const APPROACHING_PIC_LINK: &str = "https://i.ytimg.com/vi/IJJM_ccGxSQ/maxresdefault.jpg";
const WTF_PIC_LINK: &str = "https://i.redd.it/dv7afptdh9131.jpg";
const ROAD_ROLLER_PIC_LINK: &str = "https://i.ytimg.com/vi/t1y3QOIRsYs/maxresdefault.jpg";
const DOESNT_MATTER_PIC_LINK: &str =
    "https://i.kym-cdn.com/entries/icons/original/000/029/407/Screenshot_14.jpg";

pub struct SaintnosubBot<'a> {
    api: Api,
    hot_listing: &'a mut rawr::structures::listing::Listing<'a>,
    memeless_users: HashSet<User>,
}

impl<'a> SaintnosubBot<'a> {
    pub fn new(api: Api, hot_listing: &'a mut rawr::structures::listing::Listing<'a>) -> Self {
        return SaintnosubBot {
            api: api,
            hot_listing: hot_listing,
            memeless_users: HashSet::new(),
        };
    }
    async fn reply_with_photo(&mut self, message: Message, link: String) -> Result<(), Error> {
        let mut photo =
            telegram_bot::requests::SendPhoto::new(message.chat, InputFileRef::new(link));
        photo.reply_to(message.id);
        self.api.send(photo).await?;
        Ok(())
    }
    async fn drugs_message(&mut self, message: Message) -> Result<(), Error> {
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

    async fn wait_for_meme(&mut self, chat: MessageChat, user: User) -> Result<(), Error> {
        println!("Wait for meme {} ", user.first_name);
        // 15 mins to send a meme
        thread::sleep(time::Duration::from_secs(900));
        if self.memeless_users.contains(&user) {
            println!("Kick {} ", user.first_name);
            self.api.send(chat.kick(user)).await?;
        }
        Ok(())
    }

    async fn greet_users(&mut self, message: Message) -> Result<(), Error> {
        if let MessageKind::NewChatMembers { ref data, .. } = message.kind {
            let me = self.api.send(GetMe).await?;
            let mut usernames = String::new();
            for user in data {
                if user.id != me.id {
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
            for user in data {
                if user.id != me.id {
                    self.wait_for_meme(message.clone().chat, user.clone())
                        .await?;
                }
            }
        }
        Ok(())
    }
    async fn goodbye_user(&mut self, message: Message) -> Result<(), Error> {
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
    async fn parse_message(&mut self, message: Message) -> Result<(), Error> {
        if let MessageKind::Text { ref data, .. } = message.kind {
            let text = data.as_str();
            if message.from.is_bot {
                if text.contains("#game") {
                    self.reply_with_photo(message, String::from(FOOL_PIC_LINK))
                        .await?;
                }
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
