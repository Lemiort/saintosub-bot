extern crate rawr;
use std::env;

use futures::StreamExt;
use regex::Regex;
use telegram_bot::prelude::*;
use telegram_bot::{
    Api, Error, GetMe, InputFileRef, Message, MessageKind, MessageOrChannelPost, UpdateKind,
};
// use tokio::time::delay_for;

use rawr::prelude::*;

const FOOL_PIC_LINK: &str = "https://c7.hotpng.com/preview/259/820/839/dio-brando-jojo-s-bizarre-adventure-eyes-of-heaven-youtube-diamond-is-unbreakable-just-cause-thumbnail.jpg";
const APPROACHING_PIC_LINK: &str = "https://i.ytimg.com/vi/IJJM_ccGxSQ/maxresdefault.jpg";
const WTF_PIC_LINK: &str = "https://i.redd.it/dv7afptdh9131.jpg";
const ROAD_ROLLER_PIC_LINK: &str = "https://i.ytimg.com/vi/t1y3QOIRsYs/maxresdefault.jpg";
const DOESNT_MATTER_PIC_LINK: &str =
    "https://i.kym-cdn.com/entries/icons/original/000/029/407/Screenshot_14.jpg";

// async fn start_message(api: Api, message: Message) -> Result<(), Error> {
//     api.send(message.text_reply(
//         "Master Dio Brando. My stand power is Jojo memes.
//                                 Ask me kindly for meme with /jojomeme",
//     ))
//     .await?;
//     Ok(())
// }

async fn reply_with_photo(api: Api, message: Message, link: String) -> Result<(), Error> {
    let mut photo = telegram_bot::requests::SendPhoto::new(message.chat, InputFileRef::new(link));
    photo.reply_to(message.id);
    api.send(photo).await?;
    Ok(())
}

async fn drugs_message(api: Api, message: Message) -> Result<(), Error> {
    if let Some(reply_box) = message.reply_to_message {
        let value = *reply_box;
        if let MessageOrChannelPost::Message(org_message) = value {
            reply_with_photo(api, org_message, String::from(ROAD_ROLLER_PIC_LINK)).await?;
        }
    } else {
        reply_with_photo(api, message, String::from(DOESNT_MATTER_PIC_LINK)).await?;
    }
    Ok(())
}

async fn greet_users(api: Api, message: Message) -> Result<(), Error> {
    if let MessageKind::NewChatMembers { ref data, .. } = message.kind {
        let result = api.send(GetMe).await?;
        let mut self_greeting = true;
        let mut usernames = String::new();
        for user in data {
            if user.id != result.id {
                self_greeting = false;
                if let Some(login) = &user.username {
                    usernames = format!("{} @{}, ", usernames, login);
                } else {
                    usernames = format!("{} {}, ", usernames, user.first_name);
                }
            }
        }
        if self_greeting == false {
            let greeting = format!("{} мем или бан!", usernames);
            api.send(message.text_reply(greeting)).await?;
        }
    }
    Ok(())
}

async fn goodbye_user(api: Api, message: Message) -> Result<(), Error> {
    if let MessageKind::LeftChatMember { ref data, .. } = message.kind {
        let result = api.send(GetMe).await?;
        if data.id != result.id {
            let greeting = String::from("Скатерью дорожка");
            api.send(message.text_reply(greeting)).await?;
        }
    }
    Ok(())
}

async fn send_meme(
    api: Api,
    mut listing: &mut rawr::structures::listing::Listing<'_>,
    message: Message,
) -> Result<(), Error> {
    let reply = get_last_meme(&mut listing);
    let mut text = String::from(WTF_PIC_LINK);
    if let Some(title) = reply {
        text = title;
    }

    reply_with_photo(api, message, text).await?;
    Ok(())
}

async fn parse_message(api: Api, message: Message) -> Result<(), Error> {
    if let MessageKind::Text { ref data, .. } = message.kind {
        let text = data.as_str();
        if message.from.is_bot {
            if text.contains("#game") {
                reply_with_photo(api, message, String::from(FOOL_PIC_LINK)).await?;
            }
        }
    }
    Ok(())
}

fn is_jojo(name: &String, second_name: &Option<String>) -> bool {
    if let Some(second_name) = second_name {
        let re1 = Regex::new(r"^[Jj][Oo][Jj][Oo][A-Za-z]*$").unwrap();
        let match1 = re1.is_match(name);
        let re2 = Regex::new(r"^[A-Za-z]*[Jj][Oo][Jj][Oo]$").unwrap();
        let match2 = re2.is_match(second_name);
        return match1 && match2;
    } else {
        let re = Regex::new(r"^[Jj][Oo][Jj][Oo]$").unwrap();
        return re.is_match(name);
    }
}

async fn parse_photo(api: Api, message: Message) -> Result<(), Error> {
    if let MessageKind::Photo { .. } = message.kind {
        println!("There is photo from {}", message.from.first_name);
        if message.from.is_bot && is_jojo(&message.from.first_name, &message.from.last_name) {
            reply_with_photo(api, message, String::from(APPROACHING_PIC_LINK)).await?;
        }
    }
    Ok(())
}

async fn handle_message(
    api: Api,
    listing: &mut rawr::structures::listing::Listing<'_>,
    message: Message,
) -> Result<(), Error> {
    match message.kind {
        MessageKind::Text { ref data, .. } => match data.as_str() {
            // "/start" => start_message(api, message).await?,
            "/tabletki" => drugs_message(api, message).await?,
            "/jojomeme" => send_meme(api, listing, message).await?,
            "/jojomeme@saintnosubbot" => send_meme(api, listing, message).await?,
            _ => parse_message(api, message).await?,
        },
        MessageKind::NewChatMembers { .. } => greet_users(api, message).await?,
        MessageKind::LeftChatMember { .. } => goodbye_user(api, message).await?,
        MessageKind::Photo { .. } => parse_photo(api, message).await?,
        _ => (),
    };

    Ok(())
}

fn get_last_meme(listing: &mut rawr::structures::listing::Listing<'_>) -> Option<String> {
    if let Some(post) = listing.next() {
        return post.link_url();
    }
    return None;
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

    while let Some(update) = stream.next().await {
        let update = update?;
        if let UpdateKind::Message(message) = update.kind {
            handle_message(api.clone(), &mut hot_listing, message).await?;
        }
    }

    Ok(())
}
