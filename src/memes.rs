use std::{collections::VecDeque, error::Error, io, time::Duration};

use rand::seq::IndexedRandom;
use reqwest::{Client, Url};
use serde::Deserialize;

const REDDIT_FEED: &str = "https://www.reddit.com/r/ShitpostCrusaders/hot.json?limit=25&raw_json=1";
const MEME_API: &str = "https://meme-api.com/gimme/ShitpostCrusaders/25";
type FetchError = Box<dyn Error + Send + Sync>;

#[derive(Deserialize)]
struct RedditListing {
    data: RedditData,
}

#[derive(Deserialize)]
struct RedditData {
    after: Option<String>,
    children: Vec<RedditPost>,
}

#[derive(Deserialize)]
struct RedditPost {
    data: Meme,
}

#[derive(Deserialize)]
struct Meme {
    url: Option<String>,
}

#[derive(Deserialize)]
struct MemeBatch {
    memes: Vec<Meme>,
}

pub struct MemeReader {
    client: Client,
    memes: VecDeque<String>,
    after: Option<String>,
    reddit_url: String,
    fallback_url: String,
}

impl MemeReader {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: Client::builder()
                .user_agent(concat!("saintnosub-bot/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build()?,
            memes: VecDeque::new(),
            after: None,
            reddit_url: REDDIT_FEED.to_owned(),
            fallback_url: MEME_API.to_owned(),
        })
    }

    async fn fetch_reddit(&mut self) -> Result<(), FetchError> {
        let mut request = self.client.get(&self.reddit_url);
        if let Some(after) = &self.after {
            request = request.query(&[("after", after)]);
        }
        let listing: RedditListing = request.send().await?.error_for_status()?.json().await?;
        let memes = image_urls(listing.data.children.into_iter().map(|post| post.data));
        if memes.is_empty() {
            return Err(io::Error::other("Reddit returned no supported meme images").into());
        }
        self.after = listing.data.after;
        self.memes = memes;
        Ok(())
    }

    pub async fn get_meme(&mut self) -> Result<String, FetchError> {
        if let Some(url) = self.memes.pop_front() {
            return Ok(url);
        }
        if let Err(reddit_error) = self.fetch_reddit().await {
            log::warn!("Reddit feed unavailable, trying Meme API: {reddit_error}");
            let batch: MemeBatch = self
                .client
                .get(&self.fallback_url)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            self.memes = image_urls(batch.memes);
        }
        self.memes
            .pop_front()
            .ok_or_else(|| io::Error::other("Meme sources returned no supported image URLs").into())
    }

    /// Fetch a real meme and verify that its URL serves a nonempty image.
    pub async fn check(&mut self) -> Result<String, FetchError> {
        let url = self.get_meme().await?;
        let response = self.client.get(&url).send().await?.error_for_status()?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if !content_type.starts_with("image/") {
            return Err(io::Error::other(format!(
                "Meme URL returned {content_type}, expected an image"
            ))
            .into());
        }
        if response.bytes().await?.is_empty() {
            return Err(io::Error::other("Meme image is empty").into());
        }
        Ok(url)
    }
}

fn image_urls(memes: impl IntoIterator<Item = Meme>) -> VecDeque<String> {
    memes
        .into_iter()
        .filter_map(|meme| meme.url)
        .filter(|url| {
            let Ok(url) = Url::parse(url) else {
                return false;
            };
            let path = url.path().to_ascii_lowercase();
            matches!(url.scheme(), "http" | "https")
                && [".jpg", ".jpeg", ".png"]
                    .iter()
                    .any(|extension| path.ends_with(extension))
        })
        .collect()
}

pub fn get_random_pig() -> String {
    const PIGS_LINKS: &[&str] = &[
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
    PIGS_LINKS.choose(&mut rand::rng()).unwrap().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn mock_response(status: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = BufReader::new(&mut socket);
            loop {
                let mut line = String::new();
                let count = request.read_line(&mut line).unwrap();
                if count == 0 || line == "\r\n" {
                    break;
                }
            }
            socket.write_all(response.as_bytes()).unwrap();
        });
        url
    }

    #[test]
    fn filters_non_images_and_handles_missing_urls() {
        let batch: MemeBatch = serde_json::from_str(
            r#"{"memes":[
            {"url":"https://i.redd.it/a.JPG?width=100"},
            {"url":"https://i.redd.it/b.jpeg"},
            {"url":"https://i.redd.it/c.png"},
            {"url":"https://reddit.com/r/example"},
            {"url":"https://i.redd.it/video.mp4"},
            {"url":"file:///image.jpg"},{"url":null},{}
        ]}"#,
        )
        .unwrap();
        assert_eq!(image_urls(batch.memes).len(), 3);
    }

    #[tokio::test]
    async fn reddit_images_are_cached_and_last_page_can_end() {
        let mut reader = MemeReader::new().unwrap();
        reader.reddit_url = mock_response(
            "200 OK",
            r#"{"data":{"after":null,"children":[
            {"data":{"url":"https://i.redd.it/first.jpg"}},
            {"data":{"url":"https://i.redd.it/second.png"}}
        ]}}"#,
        );
        reader.fallback_url = "http://127.0.0.1:1".into();
        assert!(reader.get_meme().await.unwrap().ends_with("first.jpg"));
        assert!(reader.get_meme().await.unwrap().ends_with("second.png"));
        assert!(reader.after.is_none());
    }

    #[tokio::test]
    async fn falls_back_when_reddit_forbids_access() {
        let mut reader = MemeReader::new().unwrap();
        reader.reddit_url = mock_response("403 Forbidden", "{}");
        reader.fallback_url = mock_response(
            "200 OK",
            r#"{"memes":[{"url":"https://i.redd.it/fallback.jpg"}]}"#,
        );
        assert!(reader.get_meme().await.unwrap().ends_with("fallback.jpg"));
    }

    #[tokio::test]
    async fn empty_reddit_feed_uses_fallback() {
        let mut reader = MemeReader::new().unwrap();
        reader.reddit_url = mock_response("200 OK", r#"{"data":{"after":null,"children":[]}}"#);
        reader.fallback_url = mock_response(
            "200 OK",
            r#"{"memes":[{"url":"https://i.redd.it/fallback.png"}]}"#,
        );
        assert!(reader.get_meme().await.unwrap().ends_with("fallback.png"));
    }

    #[tokio::test]
    async fn empty_sources_return_error_instead_of_panicking() {
        let mut reader = MemeReader::new().unwrap();
        reader.reddit_url = mock_response("200 OK", r#"{"data":{"after":null,"children":[]}}"#);
        reader.fallback_url = mock_response("200 OK", r#"{"memes":[]}"#);
        assert!(reader.get_meme().await.is_err());
    }

    #[tokio::test]
    async fn failed_sources_return_error_and_can_retry() {
        let mut reader = MemeReader::new().unwrap();
        reader.reddit_url = mock_response("403 Forbidden", "{}");
        reader.fallback_url = mock_response("503 Service Unavailable", "{}");
        assert!(reader.get_meme().await.is_err());
        reader.reddit_url = mock_response(
            "200 OK",
            r#"{"data":{"after":null,"children":[{"data":{"url":"https://i.redd.it/recovered.png"}}]}}"#,
        );
        assert!(reader.get_meme().await.unwrap().ends_with("recovered.png"));
    }
}
