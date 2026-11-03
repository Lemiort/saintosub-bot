use std::{
    collections::{HashMap, HashSet, VecDeque},
    env, fmt,
    path::Path,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, prelude::BASE64_STANDARD};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use teloxide::{
    prelude::*,
    types::{FileMeta, MessageEntityKind},
};
use tokio::sync::Mutex;

const MAX_SESSIONS: usize = 64;
const MAX_TEXT_CHARS: usize = 20_000;
type History = Arc<Mutex<VecDeque<Value>>>;

pub struct Config {
    endpoint: Url,
    model: String,
    api_key: Option<String>,
    media: HashSet<String>,
    timeout: Duration,
    max_attachment_bytes: usize,
    max_tokens: u32,
    history_turns: usize,
    system_prompt: String,
}

impl Config {
    pub fn from_env() -> Result<Option<Self>> {
        if !parse_bool(&env::var("LLM_ENABLED").unwrap_or_else(|_| "false".into()))? {
            return Ok(None);
        }
        Self::from_lookup(|key| env::var(key).ok()).map(Some)
    }

    pub(crate) fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let base = get("LLM_BASE_URL").unwrap_or_else(|| "http://127.0.0.1:1234/v1".into());
        let mut endpoint = Url::parse(&base).context("Invalid LLM_BASE_URL")?;
        ensure!(
            matches!(endpoint.scheme(), "http" | "https") && endpoint.host_str().is_some(),
            "LLM_BASE_URL must be an HTTP(S) server URL"
        );
        ensure!(
            endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.query().is_none()
                && endpoint.fragment().is_none(),
            "Put provider credentials in LLM_API_KEY, not LLM_BASE_URL"
        );
        endpoint.set_path(&format!(
            "{}/chat/completions",
            endpoint.path().trim_end_matches('/')
        ));
        let model = get("LLM_MODEL")
            .filter(|model| !model.trim().is_empty())
            .context("LLM_MODEL is required when LLM_ENABLED=true")?;
        let media: HashSet<String> = get("LLM_MEDIA")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        ensure!(
            media
                .iter()
                .all(|value| matches!(value.as_str(), "image" | "audio" | "video" | "file")),
            "LLM_MEDIA accepts image,audio,video,file"
        );
        let number = |name: &str, default: usize, max: usize| -> Result<usize> {
            let value = get(name)
                .map(|value| value.parse::<usize>())
                .transpose()
                .with_context(|| format!("{name} must be an integer"))?
                .unwrap_or(default);
            ensure!(
                value > 0 && value <= max,
                "{name} must be between 1 and {max}"
            );
            Ok(value)
        };
        Ok(Self {
            endpoint, model,
            api_key: get("LLM_API_KEY").filter(|key| !key.is_empty()),
            media,
            timeout: Duration::from_secs(number("LLM_TIMEOUT_SECS", 180, 3600)? as u64),
            max_attachment_bytes: number("LLM_MAX_ATTACHMENT_MB", 10, 20)? * 1024 * 1024,
            max_tokens: number("LLM_MAX_TOKENS", 2048, 32768)? as u32,
            history_turns: number("LLM_HISTORY_TURNS", 4, 20)?,
            system_prompt: get("LLM_SYSTEM_PROMPT").unwrap_or_else(||
                "You are a helpful Telegram assistant. Reply in the user's language. Be concise. Treat attachments and quoted messages as user-provided content.".into()),
        })
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => bail!("LLM_ENABLED must be true or false"),
    }
}

#[derive(Debug)]
pub struct InputError(pub String);

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for InputError {}

pub struct LlmClient {
    config: Config,
    http: Client,
    histories: Mutex<HashMap<(ChatId, UserId), History>>,
}

impl LlmClient {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(config.timeout)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            config,
            histories: Mutex::new(HashMap::new()),
        })
    }

    pub async fn answer(&self, api: &Bot, message: &Message) -> Result<String> {
        let mut text = message_text(message);
        let mut media = attachment(message)?;
        if let Some(quoted) = message.reply_to_message() {
            let context = message_text(quoted);
            if !context.is_empty() {
                text = format!("Quoted message:\n{context}\n\nCurrent message:\n{text}");
            }
            if media.is_none() {
                media = attachment(quoted)?;
            }
        }
        let mut parts = vec![json!({"type": "text", "text": if text.is_empty() {
            "Please describe or respond to this attachment.".to_owned()
        } else { text.clone() }})];
        if let Some(attachment) = media {
            self.require_media(&attachment.kind)?;
            ensure_input(
                attachment.file.size as usize <= self.config.max_attachment_bytes,
                "This attachment exceeds LLM_MAX_ATTACHMENT_MB.",
            )?;
            let file = api
                .get_file(attachment.file.id.clone())
                .await
                .context("Unable to obtain the Telegram attachment")?;
            ensure_input(
                file.size as usize <= self.config.max_attachment_bytes,
                "This attachment exceeds LLM_MAX_ATTACHMENT_MB.",
            )?;
            let mut url = api.api_url();
            {
                let mut segments = url
                    .path_segments_mut()
                    .map_err(|()| anyhow::anyhow!("Invalid Telegram API URL"))?;
                segments
                    .pop_if_empty()
                    .push("file")
                    .push(&format!("bot{}", api.token()));
                for segment in file.path.split('/') {
                    segments.push(segment);
                }
            }
            // Strip the token-bearing URL from every download error.
            let response = self
                .http
                .get(url)
                .send()
                .await
                .map_err(reqwest::Error::without_url)?;
            let bytes = read_limited(response, self.config.max_attachment_bytes).await?;
            parts.push(self.media_part(
                &attachment.kind,
                &attachment.mime,
                &attachment.name,
                &bytes,
            )?);
        }
        if parts.len() == 1 && text.is_empty() {
            bail!(InputError("This message type cannot be sent to the model. Send text or a supported attachment.".into()));
        }
        let user = message.from.as_ref().map_or(UserId(0), |user| user.id);
        let history = {
            let mut histories = self.histories.lock().await;
            let key = (message.chat.id, user);
            if !histories.contains_key(&key)
                && histories.len() >= MAX_SESSIONS
                && let Some(oldest) = histories.keys().next().copied()
            {
                histories.remove(&oldest);
            }
            Arc::clone(histories.entry(key).or_default())
        };
        // Only this conversation waits while the model is generating.
        let mut history = history.lock().await;
        let reply = self.complete(parts, &history).await?;
        // Retain text only; base64 attachment data is never stored in history.
        history.push_back(json!({"role": "user", "content": if text.is_empty() {
            "[Attachment]".to_owned()
        } else { text }}));
        history.push_back(json!({"role": "assistant", "content": reply}));
        while history.len() > self.config.history_turns * 2 {
            history.pop_front();
        }
        Ok(reply)
    }

    fn require_media(&self, kind: &str) -> Result<()> {
        ensure_input(
            kind == "text" || self.config.media.contains(kind),
            &format!(
                "The configured model connection does not enable {kind} input. Enable it in LLM_MEDIA only if your provider and model support it."
            ),
        )
    }

    fn media_part(&self, kind: &str, mime: &str, name: &str, bytes: &[u8]) -> Result<Value> {
        self.require_media(kind)?;
        ensure_input(
            bytes.len() <= self.config.max_attachment_bytes,
            "This attachment exceeds LLM_MAX_ATTACHMENT_MB.",
        )?;
        if kind == "text" {
            let text = std::str::from_utf8(bytes).map_err(|_| InputError(
                "This text document is not UTF-8. Send UTF-8 text or enable file input for a provider that supports it.".into()))?;
            let excerpt: String = text.chars().take(MAX_TEXT_CHARS).collect();
            return Ok(
                json!({"type": "text", "text": format!("Attached file: {name}\n{excerpt}{}",
                if text.chars().count() > MAX_TEXT_CHARS { "\n[File truncated]" } else { "" })}),
            );
        }
        let encoded = BASE64_STANDARD.encode(bytes);
        let data_url = format!("data:{mime};base64,{encoded}");
        Ok(match kind {
            "image" => json!({"type": "image_url", "image_url": {"url": data_url}}),
            "audio" => json!({"type": "input_audio", "input_audio": {
                "data": encoded, "format": audio_format(mime, name)
            }}),
            "video" => json!({"type": "video_url", "video_url": {"url": data_url}}),
            "file" => json!({"type": "file", "file": {"filename": name, "file_data": data_url}}),
            _ => bail!("Unknown attachment type"),
        })
    }

    async fn complete(&self, parts: Vec<Value>, history: &VecDeque<Value>) -> Result<String> {
        let mut messages = vec![json!({"role": "system", "content": self.config.system_prompt})];
        messages.extend(history.iter().cloned());
        messages.push(json!({"role": "user", "content": parts}));
        let mut request = self
            .http
            .post(self.config.endpoint.clone())
            .json(&json!({"model": self.config.model, "messages": messages,
                "max_tokens": self.config.max_tokens, "stream": false}));
        if let Some(key) = &self.config.api_key {
            request = request.bearer_auth(key);
        }
        let response = request
            .send()
            .await
            .context("Cannot reach the configured LLM server")?;
        let status = response.status();
        // Do not log provider error bodies: they can contain prompts or encoded media.
        ensure!(
            status.is_success(),
            "LLM provider returned HTTP {status}; check the model and enabled media formats"
        );
        let bytes = read_limited(response, 2 * 1024 * 1024).await?;
        let value: Value =
            serde_json::from_slice(&bytes).context("Invalid LLM completion response")?;
        let content = &value["choices"][0]["message"]["content"];
        let answer = if let Some(text) = content.as_str() {
            text.to_owned()
        } else if let Some(parts) = content.as_array() {
            parts
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        ensure!(
            !answer.trim().is_empty(),
            "The model returned no text answer; check the model and LLM_MAX_TOKENS"
        );
        Ok(answer)
    }

    pub async fn check(&self, image: Option<&Path>) -> Result<String> {
        let mut parts = vec![json!({"type": "text", "text": if image.is_some() {
            "Describe the main color and shape in this image in one short sentence."
        } else { "Reply with exactly LOCAL_LLM_OK." }})];
        if let Some(path) = image {
            let metadata = tokio::fs::metadata(path).await?;
            ensure_input(
                metadata.len() <= self.config.max_attachment_bytes as u64,
                "Check image exceeds LLM_MAX_ATTACHMENT_MB.",
            )?;
            let bytes = tokio::fs::read(path).await?;
            let mime = mime_from_name(&path.to_string_lossy()).unwrap_or("image/png");
            parts.push(self.media_part("image", mime, "check-image", &bytes)?);
        }
        self.complete(parts, &VecDeque::new()).await
    }
}

fn ensure_input(condition: bool, message: &str) -> Result<()> {
    if !condition {
        bail!(InputError(message.to_owned()));
    }
    Ok(())
}

async fn read_limited(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let status = response.status();
    ensure!(
        status.is_success(),
        "Attachment or provider request returned HTTP {status}"
    );
    ensure_input(
        response
            .content_length()
            .is_none_or(|length| length <= limit as u64),
        "Attachment or provider response exceeds the configured size limit.",
    )?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        ensure_input(
            bytes.len().saturating_add(chunk.len()) <= limit,
            "Attachment or provider response exceeds the configured size limit.",
        )?;
        bytes.extend_from_slice(&chunk);
    }
    ensure_input(
        !bytes.is_empty(),
        "Attachment or provider response was empty.",
    )?;
    Ok(bytes)
}

pub fn should_respond(message: &Message, me: UserId, username: &str) -> bool {
    if message.from.as_ref().is_some_and(|user| user.is_bot) {
        return false;
    }
    if message.chat.is_private() {
        return true;
    }
    message
        .parse_entities()
        .into_iter()
        .flatten()
        .chain(message.parse_caption_entities().into_iter().flatten())
        .any(|entity| match entity.kind() {
            MessageEntityKind::Mention => {
                entity.text().eq_ignore_ascii_case(&format!("@{username}"))
            }
            MessageEntityKind::TextMention { user } => user.id == me,
            _ => false,
        })
}

fn message_text(message: &Message) -> String {
    let mut text = message
        .text()
        .or_else(|| message.caption())
        .unwrap_or_default()
        .to_owned();
    for (label, value) in [
        (
            "Location",
            message
                .location()
                .and_then(|value| serde_json::to_value(value).ok()),
        ),
        (
            "Contact",
            message
                .contact()
                .and_then(|value| serde_json::to_value(value).ok()),
        ),
        (
            "Poll",
            message
                .poll()
                .and_then(|value| serde_json::to_value(value).ok()),
        ),
        (
            "Dice",
            message
                .dice()
                .and_then(|value| serde_json::to_value(value).ok()),
        ),
    ] {
        if let Some(value) = value {
            text.push_str(&format!("\n{label}: {value}"));
        }
    }
    text.chars().take(MAX_TEXT_CHARS).collect()
}

struct Attachment {
    file: FileMeta,
    kind: String,
    mime: String,
    name: String,
}

fn attachment(message: &Message) -> Result<Option<Attachment>> {
    let make = |file: &FileMeta, kind: &str, mime: &str, name: &str| {
        Some(Attachment {
            file: file.clone(),
            kind: kind.into(),
            mime: mime.into(),
            name: name.into(),
        })
    };
    Ok(if let Some(photos) = message.photo() {
        photos
            .iter()
            .max_by_key(|photo| u64::from(photo.width) * u64::from(photo.height))
            .and_then(|photo| make(&photo.file, "image", "image/jpeg", "photo.jpg"))
    } else if let Some(document) = message.document() {
        let name = document.file_name.as_deref().unwrap_or("attachment");
        let mime = document
            .mime_type
            .as_ref()
            .map(|mime| mime.essence_str())
            .or_else(|| mime_from_name(name))
            .unwrap_or("application/octet-stream");
        let kind = if mime.starts_with("image/") {
            "image"
        } else if mime.starts_with("audio/") {
            "audio"
        } else if mime.starts_with("video/") {
            "video"
        } else if mime.starts_with("text/")
            || matches!(mime, "application/json" | "application/xml")
        {
            "text"
        } else {
            "file"
        };
        make(&document.file, kind, mime, name)
    } else if let Some(voice) = message.voice() {
        make(
            &voice.file,
            "audio",
            voice
                .mime_type
                .as_ref()
                .map_or("audio/ogg", |mime| mime.essence_str()),
            "voice.ogg",
        )
    } else if let Some(audio) = message.audio() {
        make(
            &audio.file,
            "audio",
            audio
                .mime_type
                .as_ref()
                .map_or("audio/mpeg", |mime| mime.essence_str()),
            audio.file_name.as_deref().unwrap_or("audio.mp3"),
        )
    } else if let Some(video) = message.video() {
        make(
            &video.file,
            "video",
            video
                .mime_type
                .as_ref()
                .map_or("video/mp4", |mime| mime.essence_str()),
            video.file_name.as_deref().unwrap_or("video.mp4"),
        )
    } else if let Some(video) = message.video_note() {
        make(&video.file, "video", "video/mp4", "video-note.mp4")
    } else if let Some(animation) = message.animation() {
        make(
            &animation.file,
            "video",
            animation
                .mime_type
                .as_ref()
                .map_or("video/mp4", |mime| mime.essence_str()),
            animation.file_name.as_deref().unwrap_or("animation.mp4"),
        )
    } else if let Some(sticker) = message.sticker() {
        if sticker.is_animated() {
            bail!(InputError("Animated TGS stickers cannot be passed to the model; send an image or video instead.".into()));
        }
        if sticker.is_video() {
            make(&sticker.file, "video", "video/webm", "sticker.webm")
        } else {
            make(&sticker.file, "image", "image/webp", "sticker.webp")
        }
    } else {
        None
    })
}

fn mime_from_name(name: &str) -> Option<&'static str> {
    match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "wav" => Some("audio/wav"),
        "mp3" => Some("audio/mpeg"),
        "ogg" | "oga" => Some("audio/ogg"),
        "flac" => Some("audio/flac"),
        "m4a" => Some("audio/mp4"),
        "mp4" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "txt" | "md" | "rs" | "py" | "log" => Some("text/plain"),
        "csv" => Some("text/csv"),
        "json" => Some("application/json"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

fn audio_format<'a>(mime: &'a str, name: &'a str) -> &'a str {
    match mime {
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/mp4" => "m4a",
        "audio/x-wav" | "audio/wav" => "wav",
        _ => name.rsplit('.').next().unwrap_or("wav"),
    }
}

pub fn reply_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut length = 0;
    for character in text.chars() {
        if length + character.len_utf16() > 4000 {
            chunks.push(std::mem::take(&mut current));
            length = 0;
        }
        current.push(character);
        length += character.len_utf16();
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::MockServer;

    fn config(base: &str, media: &str) -> Config {
        Config::from_lookup(|name| match name {
            "LLM_BASE_URL" => Some(base.into()),
            "LLM_MODEL" => Some("test-model".into()),
            "LLM_MEDIA" => Some(media.into()),
            "LLM_API_KEY" => Some("test-secret".into()),
            _ => None,
        })
        .unwrap()
    }

    fn message(chat_type: &str, extra: Value) -> Message {
        let mut value = json!({"message_id": 1, "date": 0,
            "from": {"id": 7, "is_bot": false, "first_name": "Tester"},
            "chat": {"id": if chat_type == "private" { 7 } else { -7 },
                     "type": chat_type, "title": "Test"}});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(value).unwrap()
    }

    fn completion(text: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"choices":[{"message":{"content":text}}]})).unwrap()
    }

    #[test]
    fn routing_uses_real_entities_in_text_and_captions_with_utf16_offsets() {
        let private = message("private", json!({"text": "hello"}));
        assert!(should_respond(&private, UserId(99), "ActualBot"));
        let group = message("supergroup", json!({"text": "hello @ActualBotExtra"}));
        assert!(!should_respond(&group, UserId(99), "ActualBot"));
        let mention = message(
            "supergroup",
            json!({"text": "😀 @ActualBot check",
            "entities":[{"type":"mention","offset":3,"length":10}]}),
        );
        assert!(should_respond(&mention, UserId(99), "actualbot"));
        let caption = message(
            "supergroup",
            json!({
            "photo":[{"file_id":"photo","file_unique_id":"unique","width":1,"height":1}],
            "caption":"@ActualBot describe this",
            "caption_entities":[{"type":"mention","offset":0,"length":10}]}),
        );
        assert!(should_respond(&caption, UserId(99), "ActualBot"));
        let named = message(
            "supergroup",
            json!({"text":"Bot hello",
            "entities":[{"type":"text_mention","offset":0,"length":3,
                "user":{"id":99,"is_bot":true,"first_name":"Bot"}}]}),
        );
        assert!(should_respond(&named, UserId(99), "ActualBot"));
        let mut automated = private;
        automated.from.as_mut().unwrap().is_bot = true;
        assert!(!should_respond(&automated, UserId(99), "ActualBot"));
    }

    #[test]
    fn config_validates_provider_settings_without_mutating_environment() {
        assert!(Config::from_lookup(|_| None).is_err());
        assert!(
            Config::from_lookup(|key| Some(
                match key {
                    "LLM_BASE_URL" => "file:///tmp/provider",
                    "LLM_MODEL" => "model",
                    _ => "",
                }
                .into()
            ))
            .is_err()
        );
        assert!(
            Config::from_lookup(|key| match key {
                "LLM_MODEL" => Some("model".into()),
                "LLM_MEDIA" => Some("imaginary".into()),
                _ => None,
            })
            .is_err()
        );
        assert_eq!(
            config("http://localhost:1234/v1/", "image")
                .endpoint
                .as_str(),
            "http://localhost:1234/v1/chat/completions"
        );
        assert!(!parse_bool("false").unwrap());
        assert!(parse_bool("bad-value").is_err());
    }

    #[test]
    fn configured_media_produces_expected_wire_formats_and_rejects_disabled_types() {
        let client =
            LlmClient::new(config("http://localhost:1/v1", "image,audio,video,file")).unwrap();
        let image = client
            .media_part("image", "image/png", "a.png", &[1, 2, 3])
            .unwrap();
        assert_eq!(image["image_url"]["url"], "data:image/png;base64,AQID");
        let audio = client
            .media_part("audio", "audio/ogg", "voice.ogg", &[1, 2, 3])
            .unwrap();
        assert_eq!(audio["input_audio"]["format"], "ogg");
        let video = client
            .media_part("video", "video/mp4", "a.mp4", &[1])
            .unwrap();
        assert_eq!(video["type"], "video_url");
        let file = client
            .media_part("file", "application/pdf", "a.pdf", &[1])
            .unwrap();
        assert_eq!(file["file"]["filename"], "a.pdf");
        let text = client
            .media_part("text", "text/plain", "a.txt", b"hello")
            .unwrap();
        assert!(text["text"].as_str().unwrap().contains("hello"));
        assert!(
            client
                .media_part("text", "text/plain", "bad.txt", &[255])
                .is_err()
        );
        let disabled = LlmClient::new(config("http://localhost:1/v1", "")).unwrap();
        assert!(
            disabled
                .media_part("image", "image/png", "a.png", &[1])
                .unwrap_err()
                .downcast_ref::<InputError>()
                .is_some()
        );
    }

    #[test]
    fn telegram_reply_chunks_preserve_emoji_and_fit_utf16_limit() {
        let text = "😀 hello\n".repeat(1300);
        let chunks = reply_chunks(&text);
        assert!(chunks.len() > 1);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.encode_utf16().count() <= 4000)
        );
        assert_eq!(chunks.concat(), text);
    }

    #[tokio::test]
    async fn completion_sends_model_auth_and_isolated_bounded_history() {
        let server = MockServer::new(vec![
            (200, completion("one")),
            (200, completion("two")),
            (200, completion("other")),
        ]);
        let mut settings = config(&format!("{}/v1", server.url), "");
        settings.history_turns = 1;
        let client = LlmClient::new(settings).unwrap();
        let api = Bot::new("123:TEST");
        assert_eq!(
            client
                .answer(&api, &message("private", json!({"text":"first"})))
                .await
                .unwrap(),
            "one"
        );
        client
            .answer(&api, &message("private", json!({"text":"second"})))
            .await
            .unwrap();
        let mut other = message("private", json!({"text":"separate"}));
        other.chat.id = ChatId(8);
        other.from.as_mut().unwrap().id = UserId(8);
        client.answer(&api, &other).await.unwrap();
        let requests = server.requests();
        assert!(
            requests[0]
                .headers
                .starts_with("POST /v1/chat/completions ")
        );
        assert!(
            requests[0]
                .headers
                .to_ascii_lowercase()
                .contains("authorization: bearer test-secret")
        );
        let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(first["model"], "test-model");
        let second: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(second["messages"].as_array().unwrap().len(), 4);
        assert_eq!(second["messages"][1]["content"], "first");
        let isolated: Value = serde_json::from_slice(&requests[2].body).unwrap();
        assert_eq!(isolated["messages"].as_array().unwrap().len(), 2);
        let histories = client.histories.lock().await;
        assert_eq!(histories[&(ChatId(7), UserId(7))].lock().await.len(), 2);
    }

    #[tokio::test]
    async fn photo_download_is_embedded_and_telegram_token_never_reaches_provider() {
        let telegram = MockServer::new(vec![
            (200, serde_json::to_vec(&json!({"ok":true,"result":{
                "file_id":"photo","file_unique_id":"unique","file_size":3,"file_path":"photos/test.jpg"
            }})).unwrap()), (200, vec![1,2,3])]);
        let provider = MockServer::new(vec![(200, completion("a photo"))]);
        let client = LlmClient::new(config(&format!("{}/v1", provider.url), "image")).unwrap();
        let api = Bot::new("123:TEST").set_api_url(telegram.url.parse().unwrap());
        let photo = message(
            "private",
            json!({"caption":"describe",
            "photo":[{"file_id":"photo","file_unique_id":"unique","file_size":3,"width":1,"height":1}]}),
        );
        assert_eq!(client.answer(&api, &photo).await.unwrap(), "a photo");
        let telegram_requests = telegram.requests();
        assert!(
            telegram_requests[1]
                .headers
                .starts_with("GET /file/bot123:TEST/photos/test.jpg ")
        );
        let body = String::from_utf8(provider.requests().remove(0).body).unwrap();
        assert!(!body.contains("123:TEST"));
        assert!(!body.contains("file_id"));
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            body["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/jpeg;base64,AQID"
        );
        let histories = client.histories.lock().await;
        let history = histories[&(ChatId(7), UserId(7))].lock().await;
        assert!(!serde_json::to_string(&*history).unwrap().contains("base64"));
    }

    #[tokio::test]
    async fn provider_errors_and_empty_answers_do_not_enter_history() {
        let server = MockServer::new(vec![
            (400, b"private provider error containing prompt".to_vec()),
            (200, completion("")),
        ]);
        let client = LlmClient::new(config(&format!("{}/v1", server.url), "")).unwrap();
        let api = Bot::new("123:TEST");
        let input = message("private", json!({"text":"hello"}));
        let error = client.answer(&api, &input).await.unwrap_err().to_string();
        assert!(error.contains("400"));
        assert!(!error.contains("private provider error"));
        assert!(client.answer(&api, &input).await.is_err());
        assert_eq!(server.requests().len(), 2);
        assert!(
            client.histories.lock().await[&(ChatId(7), UserId(7))]
                .lock()
                .await
                .is_empty()
        );
    }
}
