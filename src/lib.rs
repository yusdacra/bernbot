use std::{fmt::Debug, path::Path, sync::Arc};

use dashmap::DashMap;
use markov::Chain;
use rand::{prelude::IteratorRandom, Rng};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

#[cfg(feature = "discord")]
pub mod discord;

pub const POEMS: &str = include_str!("../resources/poems.txt");
pub const INSULTS: &str = include_str!("../resources/insults.txt");
pub const UMAD_JPG: &[u8] = include_bytes!("../resources/umad.jpg");

pub const HELP_TEXT: &str = "commands are:
- `help`: posts this text
- `poem`: search / get random poem or generate one
- `listen`: markov chain listener management commands
- `fuckyou`: posts funny \"u mad?\" image

use `help command` to get more information about a command";

pub const POEM_HELP_TEXT: &str = "search / get random poem or generate one

if called with no arguments it will get a random poem
arguments are counted as search keywords unless it's a subcommand

subcommands are:
- `~gen`: generates a random poem";

pub const FUCKYOU_HELP_TEXT: &str = "posts funny \"u mad?\" image";

pub const LISTEN_HELP_TEXT: &str = "markov chain listener management commands

if called with no arguments it will toggle listen status for the current channel

subcommands are:
- `getprob`: get message posting probability value
- `setprob <value>`: set message posting probability value. must be a percentage. calling it without any argument or invalid argument will set it to `5.0`.";

type MChain = Chain<SmolStr>;

#[derive(Debug, Deserialize, Serialize)]
pub struct MarkovData {
    probability: f64,
    chain: MChain,
}

impl Default for MarkovData {
    fn default() -> Self {
        Self {
            probability: 5.0,
            chain: MChain::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsultData {
    message_id: Option<SmolStr>,
    count_passed: u8,
}

impl Default for InsultData {
    fn default() -> Self {
        Self {
            message_id: None,
            count_passed: 1,
        }
    }
}

#[derive(Debug)]
pub enum BotCmd {
    SendMessage {
        text: SmolStr,
        attach: Option<(SmolStr, Vec<u8>)>,
        is_reply: bool,
    },
    DoNothing,
}

#[derive(Debug, Deserialize, Serialize)]
struct BotData {
    prefix: SmolStr,
    user_id: SmolStr,
    insult_data: DashMap<SmolStr, InsultData>,
    mchain: DashMap<SmolStr, MarkovData>,
}

#[derive(Debug, Clone)]
pub struct Bot {
    data: Arc<BotData>,
    poem_chain: Arc<MChain>,
}

impl Bot {
    pub fn new(prefix: SmolStr, user_id: SmolStr) -> Self {
        Self {
            data: Arc::new(BotData {
                prefix,
                user_id,
                insult_data: DashMap::new(),
                mchain: DashMap::new(),
            }),
            poem_chain: default_poem_chain().into(),
        }
    }

    pub fn read_from(data_path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let compressed = std::fs::read(data_path)?;
        let raw = lz4_flex::decompress_size_prepended(&compressed).unwrap();
        let data = ron::de::from_bytes(&raw).expect("failed to parse data");

        Ok(Self {
            data: Arc::new(data),
            poem_chain: default_poem_chain().into(),
        })
    }

    pub fn save_to(&self, data_path: impl AsRef<Path>) -> Result<(), std::io::Error> {
        std::fs::write(
            data_path,
            lz4_flex::compress_prepend_size(
                ron::ser::to_string_pretty(&self.data, ron::ser::PrettyConfig::default())
                    .expect("couldnt serialize")
                    .as_bytes(),
            ),
        )
    }

    pub fn markov_toggle_mark_channel(&self, channel_id: &str) -> SmolStr {
        if self.data.mchain.contains_key(channel_id) {
            self.data.mchain.remove(channel_id);
            "Will no longer listen to this channel".into()
        } else {
            self.data
                .mchain
                .insert(channel_id.into(), Default::default());
            "Will listen to this channel".into()
        }
    }

    pub fn markov_set_prob(&self, channel_id: &str, new_prob: &str) -> SmolStr {
        let prob = new_prob.parse().unwrap_or(5.0);
        if let Some(mut data) = self.data.mchain.get_mut(channel_id) {
            data.probability = prob;
            format!("Set probability to {}%", prob).into()
        } else {
            self.mark_channel_for_listen_msg()
        }
    }

    pub fn markov_get_prob(&self, channel_id: &str) -> SmolStr {
        if let Some(data) = self.data.mchain.get(channel_id) {
            format!("Probability is {}%", data.probability).into()
        } else {
            self.mark_channel_for_listen_msg()
        }
    }

    fn mark_channel_for_listen_msg(&self) -> SmolStr {
        format!(
            "First set this channel for listening, dumb human.\nA tip: you can do so with `{} listen`.",
            self.data.prefix
        ).into()
    }

    pub async fn process_args(
        &self,
        channel_id: &str,
        message_id: &str,
        message_content: &str,
        message_author: &str,
        message_reply_to: Option<&str>,
    ) -> BotCmd {
        let mut args = message_content.split_whitespace();
        #[allow(clippy::blocks_in_if_conditions)]
        if args.next() == Some(&self.data.prefix) {
            if let Some(cmd) = args.next() {
                match cmd {
                    "help" => BotCmd::SendMessage {
                        text: if let Some(subcmd) = args.next() {
                            match subcmd {
                                "poem" => POEM_HELP_TEXT.into(),
                                "listen" => LISTEN_HELP_TEXT.into(),
                                "fuckyou" => FUCKYOU_HELP_TEXT.into(),
                                "help" => HELP_TEXT.into(),
                                cmd => self.unrecognised_command(channel_id, message_id, cmd),
                            }
                        } else {
                            HELP_TEXT.into()
                        },
                        is_reply: true,
                        attach: None,
                    },
                    "poem" => BotCmd::SendMessage {
                        text: self.process_poem_command(
                            message_content
                                .trim_start_matches(self.data.prefix.as_str())
                                .trim_start_matches(" poem "),
                        ),
                        is_reply: true,
                        attach: None,
                    },
                    "fuckyou" => BotCmd::SendMessage {
                        text: SmolStr::new_inline(""),
                        is_reply: true,
                        attach: Some((SmolStr::new_inline("umad.jpg"), UMAD_JPG.to_vec())),
                    },
                    "listen" => BotCmd::SendMessage {
                        text: if let Some(subcmd) = args.next() {
                            match subcmd {
                                "setprob" => {
                                    self.markov_set_prob(channel_id, args.next().unwrap_or("5.0"))
                                }
                                "getprob" => self.markov_get_prob(channel_id),
                                _ => self.unrecognised_command(channel_id, message_id, subcmd),
                            }
                        } else {
                            self.markov_toggle_mark_channel(channel_id)
                        },
                        is_reply: true,
                        attach: None,
                    },
                    cmd => BotCmd::SendMessage {
                        text: self.unrecognised_command(channel_id, message_id, cmd),
                        is_reply: true,
                        attach: None,
                    },
                }
            } else {
                BotCmd::SendMessage {
                    text: SmolStr::new_inline("What do you want?"),
                    is_reply: true,
                    attach: None,
                }
            }
        } else if message_reply_to.map_or(false, |message_id| {
            self.has_insult_response(channel_id, message_id, message_content)
        }) {
            BotCmd::SendMessage {
                text: SmolStr::new_inline(""),
                is_reply: true,
                attach: Some((SmolStr::new_inline("umad.jpg"), UMAD_JPG.to_vec())),
            }
        } else if self.data.user_id != message_author {
            if let Some((text, is_reply)) = self
                .try_insult(&channel_id, &message_id)
                .map(|text| (text, true))
                .or_else(|| {
                    self.markov_try_gen_message(&channel_id, message_content)
                        .map(|text| (text, false))
                })
            {
                BotCmd::SendMessage {
                    text,
                    is_reply,
                    attach: None,
                }
            } else {
                BotCmd::DoNothing
            }
        } else {
            BotCmd::DoNothing
        }
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

    pub fn insult(&self, channel_id: &str, message_id: &str) -> String {
        let mut insult_data = self.insult_entry(channel_id);
        insult_data.count_passed = 1;
        insult_data.message_id = Some(message_id.into());
        choose_random_insult().to_string()
    }

    pub fn has_insult_response(
        &self,
        channel_id: &str,
        message_id: &str,
        message_content: &str,
    ) -> bool {
        self.insult_entry(channel_id).message_id.as_deref() == Some(message_id)
            && message_content.contains("fuck you")
    }

    pub fn try_insult(&self, channel_id: &str, message_id: &str) -> Option<SmolStr> {
        let mut insult_data = self.insult_entry(channel_id);
        if rand::thread_rng().gen_bool(0.05 * (insult_data.count_passed as f64) / 100.0) {
            insult_data.count_passed = 1;
            insult_data.message_id = Some(message_id.into());
            Some(choose_random_insult().into())
        } else {
            insult_data.count_passed = insult_data.count_passed.saturating_add(1);
            None
        }
    }

    pub fn markov_try_gen_message(
        &self,
        channel_id: &str,
        message_content: &str,
    ) -> Option<SmolStr> {
        if let Some(mut mlisten) = self.data.mchain.get_mut(channel_id) {
            mlisten.chain.feed(
                &message_content
                    .split_whitespace()
                    .map(SmolStr::new)
                    .collect::<Vec<_>>(),
            );
            if rand::thread_rng().gen_bool(mlisten.probability / 100.0) {
                let tokens = mlisten
                    .chain
                    .generate()
                    .into_iter()
                    .take(32)
                    .collect::<Vec<_>>();
                let mut result = String::with_capacity(tokens.iter().map(SmolStr::len).sum());
                for token in tokens {
                    result.push_str(&token);
                    result.push(' ');
                }
                return Some(result.into());
            }
        }
        None
    }

    pub fn unrecognised_command(&self, channel_id: &str, message_id: &str, cmd: &str) -> SmolStr {
        format!(
            "{}`{}` isn't even a command.",
            self.insult(channel_id, message_id),
            cmd
        )
        .into()
    }

    pub fn process_poem_command(&self, keywords: &str) -> SmolStr {
        let poem_chain = &self.poem_chain;

        if keywords.is_empty() {
            choose_random_poem().into()
        } else if keywords == "~gen" {
            let mut output = String::new();
            let some_tokens = poem_chain.generate();

            let mut rng = rand::thread_rng();
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
        } else {
            let ranker = fuzzy_matcher::skim::SkimMatcherV2::default();
            let mut ranked = POEMS
                .split('-')
                .filter_map(|choice| {
                    let score = ranker.fuzzy(choice, keywords, false)?.0;
                    if score > 100 {
                        Some((choice, score))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            ranked.sort_by_key(|(_, k)| *k);
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
