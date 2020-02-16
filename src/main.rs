extern crate rawr;
use std::env;
use std::time::Duration;

use futures::StreamExt;
use telegram_bot::prelude::*;
use telegram_bot::{Api, Error, InputFileRef, Message, MessageKind, ParseMode, UpdateKind};
use tokio::time::delay_for;

use rawr::prelude::*;

async fn test_message(api: Api, message: Message) -> Result<(), Error> {
    api.send(message.text_reply("Simple message")).await?;

    let mut reply = message.text_reply("`Markdown message`");
    api.send(reply.parse_mode(ParseMode::Markdown)).await?;

    let mut reply = message.text_reply("<b>Bold HTML message</b>");

    api.send(reply.parse_mode(ParseMode::Html)).await?;
    Ok(())
}

async fn test_preview(api: Api, message: Message) -> Result<(), Error> {
    api.send(message.text_reply("Message with preview https://telegram.org"))
        .await?;

    let mut reply = message.text_reply("Message without preview https://telegram.org");

    api.send(reply.disable_preview()).await?;
    Ok(())
}

async fn test_reply(api: Api, message: Message) -> Result<(), Error> {
    api.send(message.text_reply("Reply to message")).await?;
    api.send(message.chat.text("Text to message chat")).await?;

    api.send(message.from.text("Private text")).await?;
    Ok(())
}

async fn test_forward(api: Api, message: Message) -> Result<(), Error> {
    api.send(message.forward(&message.chat)).await?;

    api.send(message.forward(&message.from)).await?;
    Ok(())
}

async fn test_edit_message(api: Api, message: Message) -> Result<(), Error> {
    let message1 = api.send(message.text_reply("Round 1")).await?;

    delay_for(Duration::from_secs(2)).await;

    let message2 = api.send(message1.edit_text("Round 2")).await?;

    delay_for(Duration::from_secs(4)).await;

    api.send(message2.edit_text("Round 3")).await?;
    Ok(())
}

async fn test_get_chat(api: Api, message: Message) -> Result<(), Error> {
    let chat = api.send(message.chat.get_chat()).await?;
    api.send(chat.text(format!("Chat id {}", chat.id())))
        .await?;
    Ok(())
}

async fn test_get_chat_administrators(api: Api, message: Message) -> Result<(), Error> {
    let administrators = api.send(message.chat.get_administrators()).await?;
    let mut response = Vec::new();
    for member in administrators {
        response.push(member.user.first_name.clone())
    }
    api.send(message.text_reply(format!("Administrators: {}", response.join(", "))))
        .await?;
    Ok(())
}

async fn test_get_chat_members_count(api: Api, message: Message) -> Result<(), Error> {
    let count = api.send(message.chat.get_members_count()).await?;
    api.send(message.text_reply(format!("Members count: {}", count)))
        .await?;
    Ok(())
}

async fn test_get_chat_member(api: Api, message: Message) -> Result<(), Error> {
    let member = api.send(message.chat.get_member(&message.from)).await?;
    let first_name = member.user.first_name.clone();
    let status = member.status;
    api.send(message.text_reply(format!("Member {}, status {:?}", first_name, status)))
        .await?;
    Ok(())
}

async fn test_get_user_profile_photos(api: Api, message: Message) -> Result<(), Error> {
    let photos = api.send(message.from.get_user_profile_photos()).await?;

    api.send(message.text_reply(format!("Found photos: {}", photos.total_count)))
        .await?;
    Ok(())
}

async fn test_leave(api: Api, message: Message) -> Result<(), Error> {
    api.send(message.chat.leave()).await?;
    Ok(())
}

async fn test_meme(api: Api, message: Message) -> Result<(), Error> {
    let reply = get_last_meme();
    let mut text = String::from("https://i.redd.it/dv7afptdh9131.jpg");
    if let Some(title) = reply {
        text = title;
    }

    let chat = message.chat.clone();
    let photo = chat.photo(InputFileRef::new(text));
    api.send(photo).await?;
    Ok(())
}

async fn test(api: Api, message: Message) -> Result<(), Error> {
    match message.kind {
        MessageKind::Text { ref data, .. } => match data.as_str() {
            // "/message" => test_message(api, message).await?,
            // "/preview" => test_preview(api, message).await?,
            // "/reply" => test_reply(api, message).await?,
            // "/forward" => test_forward(api, message).await?,
            // "/edit-message" => test_edit_message(api, message).await?,
            // "/get_chat" => test_get_chat(api, message).await?,
            // "/get_chat_administrators" => test_get_chat_administrators(api, message).await?,
            // "/get_chat_members_count" => test_get_chat_members_count(api, message).await?,
            // "/get_chat_member" => test_get_chat_member(api, message).await?,
            // "/get_user_profile_photos" => test_get_user_profile_photos(api, message).await?,
            // "/leave" => test_leave(api, message).await?,
            "/jojomeme" => test_meme(api, message).await?,
            _ => (),
        },
        _ => (),
    };

    Ok(())
}

fn get_last_meme() -> Option<String> {
    // Creates a new client to access the reddit API. You need to set a user agent so Reddit knows
    // who is using this client.
    let client = RedditClient::new(
        "linux:saintnosubbot:v0.0.1 (by /u/Lemiort)",
        AnonymousAuthenticator::new(),
    );
    // Access the subreddit /r/ShitPostCrusaders.
    let subreddit = client.subreddit("ShitPostCrusaders");
    // Gets the hot listing of /r/ShitPostCrusaders. If the API request fails, we will panic with `expect`.
    let hot_listing = subreddit
        .new(ListingOptions::default())
        .expect("Could not fetch post listing!");
    // Iterates through the top 50 posts of /r/ShitPostCrusaders. If you do not `take(n)`, this iterator will
    // continue forever!

    // let posts = hot_listing.take(50);
    // for post in posts {
    //     println!("{}", post.title());
    // }
    let last_post = hot_listing.last();

    if let Some(post) = last_post {
        return post.link_url();
    }
    return None;
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");

    let api = Api::new(token);
    let mut stream = api.stream();

    while let Some(update) = stream.next().await {
        let update = update?;
        if let UpdateKind::Message(message) = update.kind {
            test(api.clone(), message).await?;
        }
    }

    Ok(())
}
