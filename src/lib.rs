use std::{
    error::Error,
    fmt::{self, Debug, Display, Formatter},
    ops::Not,
    path::Path,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use dashmap::DashMap;
use markov::Chain;
use rand::{
    prelude::{IteratorRandom, SmallRng},
    Rng, SeedableRng,
};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

#[cfg(feature = "discord")]
pub mod discord;

pub const AUTO_SAVE_PERIOD: u64 = 60 * 60; // save every hour
pub const PREFIX_DEF: &str = "b/";
pub const PRESENCE_DEF: &str = "G-go for it, yay. Mii, nipah~☆";
pub const CHANNEL_MARK_MSG: &str =
    "First set this channel for listening, dumb human.\nA tip: you can do so with `listen`.";
pub const NOT_ENOUGH_PERMS: &str = "Foolish human, you don't have enough permissions to do this.";

pub const POEMS: &str = include_str!("../resources/poems.txt");
pub const INSULTS: &str = include_str!("../resources/insults.txt");
pub const UMAD_JPG: &[u8] = include_bytes!("../resources/umad.jpg");

pub const HELP_TEXT: &str = "commands are:
- `help`: posts this text
- `poem`: search / get random poem
- `gen`: generate stuff from markov chains
- `listen`: markov chain listener management commands
- `fuckyou`: posts funny \"u mad?\" image

use `help command` to get more information about a command";

pub const GEN_HELP_TEXT: &str = "generate stuff from markov chains

if called with no arguments it will generate random text using the channel's markov chain
if called with a user id it will generate random text using the user's markov chain in this channel

subcommands are:
- `poem`: generates a random poem";

pub const POEM_HELP_TEXT: &str = "search / get random poem or generate one

if called with no arguments it will get a random poem
arguments are counted as search keywords";

pub const FUCKYOU_HELP_TEXT: &str = "posts funny \"u mad?\" image";

pub const LISTEN_HELP_TEXT: &str = "markov chain listener management commands

if called with no arguments it will toggle listen status for the current channel

subcommands are:
- `getprob`: get message posting probability value
- `setprob <value>`: set message posting probability value. must be a percentage. calling it without any argument or invalid argument will set it to `5.0`.";

type MChain = Chain<SmolStr>;

#[async_trait]
pub trait Handler: Send + Sync {
    type Error;

    async fn send_message(
        &self,
        text: &str,
        attach: Option<(&str, Vec<u8>)>,
        reply: bool,
    ) -> Result<SmolStr, BotError<Self::Error>>;

    async fn author_has_manage_perm(&self) -> Result<bool, BotError<Self::Error>>;

    fn referenced_id(&self) -> Option<&str>;
    fn id(&self) -> &str;
    fn author(&self) -> &str;
    fn content(&self) -> &str;
    fn channel_id(&self) -> &str;
    fn guild_id(&self) -> Option<&str>;
}

#[derive(Debug)]
pub enum BotError<E> {
    Handler(E),
}

impl<E: Display> Display for BotError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BotError::Handler(err) => write!(f, "error occured in handler: {}", err),
        }
    }
}

impl<E> From<E> for BotError<E> {
    fn from(err: E) -> Self {
        BotError::Handler(err)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MarkovData {
    #[serde(default)]
    probability: f64,
    #[serde(default)]
    chain: MChain,
    #[serde(default)]
    per_user: DashMap<SmolStr, MChain>,
    #[serde(default)]
    enabled: bool,
}

impl Default for MarkovData {
    fn default() -> Self {
        Self {
            probability: 5.0,
            chain: MChain::new(),
            per_user: DashMap::new(),
            enabled: false,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsultData {
    #[serde(default)]
    message_id: Option<SmolStr>,
    #[serde(default)]
    count_passed: u8,
    #[serde(default)]
    enabled: bool,
}

impl Default for InsultData {
    fn default() -> Self {
        Self {
            message_id: None,
            count_passed: 1,
            enabled: false,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct BotData {
    #[serde(default)]
    user_id: SmolStr,
    #[serde(default)]
    insult_data: DashMap<SmolStr, InsultData>,
    #[serde(default)]
    mchain: DashMap<SmolStr, MarkovData>,
    #[serde(default)]
    prefix: DashMap<SmolStr, SmolStr>,
}

#[derive(Debug, Clone)]
pub struct Bot {
    data: Arc<BotData>,
    poem_chain: Arc<MChain>,
}

impl Bot {
    pub fn new(user_id: SmolStr) -> Self {
        Self {
            data: Arc::new(BotData {
                user_id,
                insult_data: DashMap::new(),
                mchain: DashMap::new(),
                prefix: DashMap::new(),
            }),
            poem_chain: default_poem_chain().into(),
        }
    }

    pub async fn read_from(data_path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let compressed = tokio::fs::read(data_path).await?;
        let raw = lz4_flex::decompress_size_prepended(&compressed).unwrap();
        let data = ron::de::from_bytes(&raw).expect("failed to parse data");

        Ok(Self {
            data: Arc::new(data),
            poem_chain: default_poem_chain().into(),
        })
    }

    pub fn start_autosave_task(&self, data_path: impl AsRef<Path>) {
        let bot = self.clone();
        let data_path = data_path.as_ref().to_owned();
        tokio::spawn(async move {
            loop {
                if let Err(err) = bot.save_to(&data_path).await {
                    tracing::error!("couldnt save bot data: {}", err);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(AUTO_SAVE_PERIOD)).await;
            }
        });
    }

    pub async fn save_to(&self, data_path: impl AsRef<Path>) -> Result<(), std::io::Error> {
        tokio::fs::write(
            data_path,
            lz4_flex::compress_prepend_size(
                ron::ser::to_string(&self.data)
                    .expect("couldnt serialize")
                    .as_bytes(),
            ),
        )
        .await
    }

    pub fn markov_toggle_mark_channel(&self, channel_id: &str) -> SmolStr {
        let mut m = self.data.mchain.entry(channel_id.into()).or_default();
        m.enabled = m.enabled.not();
        if m.enabled {
            SmolStr::new_inline("marked channel")
        } else {
            SmolStr::new_inline("unmarked channel")
        }
    }

    pub fn markov_set_prob(&self, channel_id: &str, new_prob: &str) -> SmolStr {
        let prob = new_prob.parse().unwrap_or(5).min(100).max(0);
        if let Some(mut data) = self.data.mchain.get_mut(channel_id) {
            data.probability = prob as f64;
            format!("Set probability to {}%", prob).into()
        } else {
            CHANNEL_MARK_MSG.into()
        }
    }

    pub fn markov_get_prob(&self, channel_id: &str) -> SmolStr {
        if let Some(data) = self.data.mchain.get(channel_id) {
            format!("Probability is {}%", data.probability).into()
        } else {
            CHANNEL_MARK_MSG.into()
        }
    }

    pub async fn process_args<E: Error>(
        &self,
        handler: &dyn Handler<Error = E>,
    ) -> Result<(), BotError<E>> {
        let context_id = handler.guild_id().unwrap_or_else(|| handler.channel_id());
        let prefix = self
            .data
            .prefix
            .get(context_id)
            .map_or(PREFIX_DEF.into(), |a| a.clone());
        #[allow(clippy::blocks_in_if_conditions)]
        if let Some(args) = handler.content().strip_prefix(prefix.as_str()) {
            let mut args = args.split_whitespace();
            if let Some(cmd) = args.next() {
                match cmd {
                    "help" => {
                        let mut insulted = false;
                        let text = if let Some(subcmd) = args.next() {
                            match subcmd {
                                "poem" => POEM_HELP_TEXT.into(),
                                "listen" => LISTEN_HELP_TEXT.into(),
                                "fuckyou" => FUCKYOU_HELP_TEXT.into(),
                                "gen" => GEN_HELP_TEXT.into(),
                                cmd => {
                                    insulted = true;
                                    self.unrecognised_command(cmd)
                                }
                            }
                        } else {
                            HELP_TEXT.into()
                        };
                        let id = handler.send_message(&text, None, true).await?;
                        if insulted {
                            self.insult(handler.channel_id(), id);
                        }
                    }
                    "set" => {
                        let mut insulted = false;
                        let text = if handler.author_has_manage_perm().await? {
                            if let Some(subcmd) = args.next() {
                                match subcmd {
                                    "prefix" => {
                                        if let Some(value) = args.next() {
                                            self.data
                                                .prefix
                                                .insert(context_id.into(), value.into());
                                            format!("prefix is now `{}`.", value).into()
                                        } else {
                                            SmolStr::new_inline("no value")
                                        }
                                    }
                                    "insult" => {
                                        let mut m = self.insult_entry(context_id);
                                        if m.enabled {
                                            m.enabled = false;
                                            SmolStr::new_inline("turned off insults")
                                        } else {
                                            m.enabled = true;
                                            SmolStr::new_inline("turned on insults")
                                        }
                                    }
                                    cmd => {
                                        insulted = true;
                                        self.unrecognised_command(cmd)
                                    }
                                }
                            } else {
                                insulted = true;
                                self.unrecognised_command(cmd)
                            }
                        } else {
                            SmolStr::new_inline("not enough permissions")
                        };

                        let id = handler.send_message(&text, None, true).await?;
                        if insulted {
                            self.insult(handler.channel_id(), id);
                        }
                    }
                    "poem" => {
                        let text = self.process_poem_command(
                            args.map(|c| {
                                let mut s = c.to_owned();
                                s.push(' ');
                                s
                            })
                            .collect::<String>()
                            .trim_end(),
                        );
                        handler.send_message(&text, None, true).await?;
                    }
                    "fuckyou" => {
                        handler
                            .send_message("", Some(("umad.jpg", UMAD_JPG.to_vec())), true)
                            .await?;
                    }
                    "gen" => {
                        let text = if let Some(subcmd) = args.next() {
                            match subcmd {
                                "poem" => self.generate_poem(),
                                "token" => {
                                    if let Some(token) = args.next() {
                                        self.gen_message(handler.channel_id(), Some(token.into()))
                                    } else {
                                        SmolStr::new_inline("put a token")
                                    }
                                }
                                user => self.gen_user_message(handler.channel_id(), user),
                            }
                        } else {
                            self.gen_message(handler.channel_id(), None)
                        };
                        handler.send_message(&text, None, true).await?;
                    }
                    "listen" => {
                        let mut insulted = false;
                        let text = if let Some(subcmd) = args.next() {
                            match subcmd {
                                "prob" => {
                                    if let Some(new_prob) = args.next() {
                                        if handler.author_has_manage_perm().await? {
                                            self.markov_set_prob(handler.channel_id(), new_prob)
                                        } else {
                                            NOT_ENOUGH_PERMS.into()
                                        }
                                    } else {
                                        self.markov_get_prob(handler.channel_id())
                                    }
                                }
                                "clear" => {
                                    if handler.author_has_manage_perm().await? {
                                        self.data.mchain.remove(context_id);
                                        SmolStr::new_inline("cleared data")
                                    } else {
                                        NOT_ENOUGH_PERMS.into()
                                    }
                                }
                                cmd => {
                                    insulted = true;
                                    self.unrecognised_command(cmd)
                                }
                            }
                        } else if handler.author_has_manage_perm().await? {
                            self.markov_toggle_mark_channel(handler.channel_id())
                        } else {
                            NOT_ENOUGH_PERMS.into()
                        };
                        let id = handler.send_message(&text, None, true).await?;
                        if insulted {
                            self.insult(handler.channel_id(), id);
                        }
                    }
                    cmd => {
                        let id = handler
                            .send_message(&self.unrecognised_command(cmd), None, true)
                            .await?;
                        self.insult(handler.channel_id(), id);
                    }
                }
            } else {
                handler
                    .send_message("What do you want?", None, true)
                    .await?;
            }
        } else if self.data.user_id != handler.author() {
            let markov = self.markov_try_gen_message(
                handler.channel_id(),
                handler.content(),
                handler.author(),
            );
            if handler.referenced_id().map_or(false, |message_id| {
                self.has_insult_response(handler.channel_id(), message_id, handler.content())
            }) {
                handler
                    .send_message("", Some(("umad.jpg", UMAD_JPG.to_vec())), true)
                    .await?;
            } else if let Some(text) = self.try_insult(handler.channel_id()) {
                let id = handler.send_message(&text, None, true).await?;
                self.insult(handler.channel_id(), id);
            } else if let Some((text, is_reply)) = markov {
                let mut rng = rand::rngs::SmallRng::from_entropy();
                handler.send_message(&text, None, is_reply).await?;
                while let Some((text, is_reply)) = self.markov_try_gen_message(
                    handler.channel_id(),
                    handler.content(),
                    handler.author(),
                ) {
                    if rng.gen_bool(1.0 / 10.0) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    handler.send_message(&text, None, is_reply).await?;
                }
            }
        }
        Ok(())
    }

    pub fn insult_entry(
        &self,
        channel_id: &str,
    ) -> dashmap::mapref::one::RefMut<SmolStr, InsultData> {
        if !self.data.insult_data.contains_key(channel_id) {
            self.data
                .insult_data
                .insert(channel_id.into(), Default::default());
        }
        self.data.insult_data.get_mut(channel_id).unwrap()
    }

    pub fn insult(&self, channel_id: &str, message_id: SmolStr) {
        let mut insult_data = self.insult_entry(channel_id);
        insult_data.count_passed = 1;
        insult_data.message_id = Some(message_id);
    }

    pub fn has_insult_response(
        &self,
        channel_id: &str,
        message_id: &str,
        message_content: &str,
    ) -> bool {
        message_content.contains("fuck you")
            && self
                .data
                .insult_data
                .get(channel_id)
                .map_or(false, |d| d.message_id.as_deref() == Some(message_id))
    }

    pub fn try_insult(&self, channel_id: &str) -> Option<SmolStr> {
        let mut insult_data = self.insult_entry(channel_id);
        if insult_data.enabled
            && get_rng().gen_bool(0.05 * (insult_data.count_passed as f64) / 100.0)
        {
            Some(choose_random_insult().into())
        } else {
            insult_data.count_passed = insult_data.count_passed.saturating_add(1);
            None
        }
    }

    pub fn gen_user_message(&self, channel_id: &str, message_author: &str) -> SmolStr {
        if let Some(mlisten) = self.data.mchain.get(channel_id) {
            if let Some(chain) = mlisten.per_user.get(message_author) {
                let tokens = chain.generate();
                let mut result = String::with_capacity(tokens.iter().map(SmolStr::len).sum());
                for token in tokens {
                    result.push_str(&token);
                    result.push(' ');
                }
                result.into()
            } else {
                "User has no messages recorded".into()
            }
        } else {
            CHANNEL_MARK_MSG.into()
        }
    }

    pub fn gen_message(&self, channel_id: &str, token: Option<SmolStr>) -> SmolStr {
        if let Some(mlisten) = self.data.mchain.get(channel_id) {
            let tokens = if let Some(token) = token {
                mlisten.chain.generate_from_token(token)
            } else {
                mlisten.chain.generate()
            };
            let mut result = String::with_capacity(tokens.iter().map(SmolStr::len).sum());
            for token in tokens {
                result.push_str(&token);
                result.push(' ');
            }
            result.into()
        } else {
            CHANNEL_MARK_MSG.into()
        }
    }

    pub fn markov_try_gen_message(
        &self,
        channel_id: &str,
        message_content: &str,
        message_author: &str,
    ) -> Option<(SmolStr, bool)> {
        if let Some(mut mlisten) = self.data.mchain.get_mut(channel_id) {
            let mut tokens = message_content
                .split_whitespace()
                .map(SmolStr::new)
                .collect::<Vec<_>>();
            mlisten.chain.feed(&tokens);
            mlisten
                .per_user
                .entry(message_author.into())
                .or_default()
                .feed(&tokens);
            let mut rng = get_rng();
            if mlisten.enabled && rng.gen_bool(mlisten.probability / 100.0) {
                let is_reply = rng.gen_bool(1.0 / 5.0);
                let start_token = if tokens.is_empty().not() && is_reply {
                    tokens.remove(rng.gen_range(0..tokens.len()))
                } else {
                    mlisten.chain.iter_for(1).next().unwrap().pop().unwrap()
                };

                let mut tokens = mlisten
                    .chain
                    .generate_from_token(start_token.clone())
                    .into_iter()
                    .map(|s| typo(s, &mut rng))
                    .collect::<Vec<_>>();

                if tokens.is_empty() || (tokens.len() == 1 && tokens[0] == start_token) {
                    tokens.clear();
                    tokens.append(
                        &mut mlisten
                            .chain
                            .generate()
                            .into_iter()
                            .map(|s| typo(s, &mut rng))
                            .collect::<Vec<_>>(),
                    );
                }

                tokens.truncate(rng.gen_range(16..32));

                let mut result = String::with_capacity(tokens.iter().map(SmolStr::len).sum());
                for token in tokens {
                    result.push_str(&token);
                    result.push(' ');
                }
                return Some((result.into(), is_reply));
            }
        }
        None
    }

    pub fn unrecognised_command(&self, cmd: &str) -> SmolStr {
        format!("{}`{}` isn't a command.", choose_random_insult(), cmd).into()
    }

    pub fn generate_poem(&self) -> SmolStr {
        let poem_chain = &self.poem_chain;

        let mut output = String::new();
        let some_tokens = poem_chain.generate();

        let mut rng = get_rng();
        let start_token = some_tokens
            .iter()
            .filter(|c| c.chars().next().unwrap().is_uppercase())
            .choose(&mut rng)
            .unwrap()
            .clone();
        let seperate_by = rng.gen_range(2..=3);
        let poem_lines = rng.gen_range(6..=8);
        let is_sentence_end = |c| matches!(c, '.' | '!' | '?');
        let mut sentences = Vec::with_capacity(poem_lines);
        let mut sentence = Vec::new();
        let mut sentence_count = 0;
        for token in poem_chain.generate_from_token(start_token) {
            if sentence_count > 7 {
                break;
            }
            if token.ends_with(is_sentence_end) {
                sentence_count += 1;
                sentence.push(token);
                sentences.push(sentence.drain(..).collect::<Vec<_>>());
            } else {
                sentence.push(token);
            }
        }
        for (index, sentence) in sentences.into_iter().enumerate() {
            for word in sentence {
                output.push_str(&word);
                output.push(' ');
            }
            output.push('\n');
            if index % seperate_by == 0 {
                output.push('\n');
            }
        }
        output.into()
    }

    pub fn process_poem_command(&self, keywords: &str) -> SmolStr {
        if keywords.is_empty() {
            choose_random_poem().into()
        } else {
            let ranker = fuzzy_matcher::skim::SkimMatcherV2::default();
            let mut ranked = POEMS
                .split('-')
                .filter_map(|choice| {
                    let score = ranker.fuzzy(choice, keywords, false)?.0;
                    (score > 10).then(|| (choice, score))
                })
                .collect::<Vec<_>>();
            ranked.sort_unstable_by_key(|(_, k)| *k);
            if let Some((result, _)) = ranked.last() {
                (*result).into()
            } else {
                "No poem with those words. Try again, maybe a miracle will occur.".into()
            }
        }
    }
}

pub fn default_poem_chain() -> MChain {
    let mut chain = Chain::new();
    chain.feed(
        &POEMS
            .replace('-', "")
            .split_whitespace()
            .map(SmolStr::new)
            .collect::<Vec<_>>(),
    );
    chain
}

pub fn choose_random_poem() -> &'static str {
    POEMS
        .split('-')
        .choose(&mut rand::thread_rng())
        .expect("always something in poems")
}

pub fn choose_random_insult() -> &'static str {
    INSULTS
        .split('-')
        .choose(&mut rand::thread_rng())
        .expect("always something in insults")
}

fn typo(s: SmolStr, rng: &mut impl rand::Rng) -> SmolStr {
    let mut chars = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let ch = if rng.gen_bool(0.5 / 100.0) {
            rng.sample(rand::distributions::Alphanumeric) as char
        } else {
            ch
        };
        chars.push(ch);
    }
    chars.into_iter().collect()
}

fn get_rng() -> SmallRng {
    SmallRng::from_entropy()
}

#[macro_export]
macro_rules! perr {
    ($res:expr) => {
        if let Err(err) = $res {
            tracing::error!("{}", err);
        }
    };
    ($res:expr, |$val:ident| $do:expr) => {
        match $res {
            Ok($val) => $do,
            Err(err) => tracing::error!("{}", err),
        }
    };
}
