use futures::executor::block_on;
use rand::seq::SliceRandom;
use roux::util::error::RouxError;
use roux::util::FeedOption;
use roux::Subreddit;

const MEMES_COUNT: u32 = 25;

pub struct MemeReader {
    subreddit: roux::subreddit::Subreddit,
    listing: roux::subreddit::responses::submissions::Submissions,
    memes_buffer: Vec<String>,
    memes_iter: usize,
}

impl MemeReader {
    pub fn new() -> Self {
        let subreddit = Subreddit::new("ShitpostCrusaders");
        let result = block_on(subreddit.hot(MEMES_COUNT, None));
        match result {
            Ok(hot) => {
                let iter = hot.data.children.iter();
                let mut memes_buffer = Vec::new();
                for value in iter {
                    let link = value.data.url.as_ref().unwrap().clone();
                    memes_buffer.push(link);
                }
                return MemeReader {
                    subreddit: subreddit,
                    listing: hot,
                    memes_buffer: memes_buffer,
                    memes_iter: 0,
                };
            }
            Err(answer) => match answer {
                RouxError::Network(e) => panic!("network error: {}", e),
                RouxError::Parse(_) => panic!("parsing error"),
                RouxError::Status(_) => panic!("api error"),
            },
        };
    }

    pub fn get_meme(&mut self) -> String {
        let result = self.memes_buffer[self.memes_iter].clone();
        self.memes_iter += 1;
        if self.memes_iter == self.memes_buffer.len() {
            // get memes after current listing
            let after = self.listing.data.after.as_ref().unwrap().clone();
            let options = FeedOption::new().after(&after);
            let hot = block_on(self.subreddit.hot(MEMES_COUNT, Some(options))).unwrap();
            // extract memes to buffer
            let iter = hot.data.children.iter();
            let mut memes_buffer = Vec::new();
            for value in iter {
                let link = value.data.url.as_ref().unwrap().clone();
                memes_buffer.push(link);
            }
            // override current object
            self.listing = hot;
            self.memes_buffer = memes_buffer;
            self.memes_iter = 0;
        }
        // filter for images
        if result.ends_with(".jpg") || result.ends_with(".png") {
            return result;
        } else {
            return self.get_meme();
        }
    }
}

pub fn get_random_pig() -> String {
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
    let random_pig_link = *PIGS_LINKS.choose(&mut rand::thread_rng()).unwrap();
    return String::from(random_pig_link.clone());
}
