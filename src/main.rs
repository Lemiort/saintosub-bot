extern crate rawr;
use std::env;
// use std::time::Duration;

use futures::StreamExt;
use telegram_bot::prelude::*;
use telegram_bot::{Api, Error, GetMe, InputFileRef, Message, MessageKind, UpdateKind};
// use tokio::time::delay_for;

use rawr::prelude::*;

// async fn test_message(api: Api, message: Message) -> Result<(), Error> {
//     api.send(message.text_reply("Simple message")).await?;

//     let mut reply = message.text_reply("`Markdown message`");
//     api.send(reply.parse_mode(ParseMode::Markdown)).await?;

//     let mut reply = message.text_reply("<b>Bold HTML message</b>");

//     api.send(reply.parse_mode(ParseMode::Html)).await?;
//     Ok(())
// }

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

async fn test_meme(
    api: Api,
    mut listing: &mut rawr::structures::listing::Listing<'_>,
    message: Message,
) -> Result<(), Error> {
    let reply = get_last_meme(&mut listing);
    let mut text = String::from("https://i.redd.it/dv7afptdh9131.jpg");
    if let Some(title) = reply {
        text = title;
    }

    let mut photo = telegram_bot::requests::SendPhoto::new(message.chat, InputFileRef::new(text));
    photo.reply_to(message.id);
    api.send(photo).await?;
    Ok(())
}

async fn test(
    api: Api,
    listing: &mut rawr::structures::listing::Listing<'_>,
    message: Message,
) -> Result<(), Error> {
    match message.kind {
        MessageKind::Text { ref data, .. } => match data.as_str() {
            // "/message" => test_message(api, message).await?,
            "/jojomeme" => test_meme(api, listing, message).await?,
            "/jojomeme@saintnosubbot" => test_meme(api, listing, message).await?,
            _ => (),
        },
        MessageKind::NewChatMembers { .. } => greet_users(api, message).await?,
        MessageKind::LeftChatMember { .. } => goodbye_user(api, message).await?,
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
            test(api.clone(), &mut hot_listing, message).await?;
        }
    }

    Ok(())
}
