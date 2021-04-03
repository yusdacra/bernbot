use std::{fmt::Debug, path::Path, str::SplitWhitespace, sync::Arc};

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

type MChain = Chain<String>;

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
    SendText(SmolStr, bool),
    SendAttachment { name: SmolStr, data: Vec<u8> },
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

    pub fn markov_mark_channel(&self, channel_id: &str) -> SmolStr {
        if self.data.mchain.contains_key(channel_id) {
            "Channel is already marked for listening, you fool.".into()
        } else {
            self.data
                .mchain
                .insert(channel_id.into(), Default::default());
            "Will listen to this channel".into()
        }
    }

    pub fn markov_unmark_channel(&self, channel_id: &str) -> SmolStr {
        self.data.mchain.remove(channel_id);
        "Will no longer listen to this channel".into()
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
            "First set a channel to listen in, dumb human.\nA tip: you can do so with `{} listen mark`.",
            self.data.prefix
        ).into()
    }

    pub fn process_args(
        &self,
        channel_id: &str,
        message_id: &str,
        message_content: &str,
        message_author: &str,
        message_reply_to: Option<&str>,
    ) -> BotCmd {
        let mut args = message_content.split_whitespace();
        if let Some("bern") = args.next() {
            if let Some(cmd) = args.next() {
                match cmd {
                    "poem" => BotCmd::SendText(self.process_poem_command(args), true),
                    "fuckyou" => BotCmd::SendAttachment {
                        name: SmolStr::new_inline("umad.jpg"),
                        data: UMAD_JPG.to_vec(),
                    },
                    "listen" => BotCmd::SendText(
                        if let Some(subcmd) = args.next() {
                            match subcmd {
                                "mark" => self.markov_mark_channel(channel_id),
                                "unmark" => self.markov_unmark_channel(channel_id),
                                "setprob" => {
                                    self.markov_set_prob(channel_id, args.next().unwrap_or("5.0"))
                                }
                                "getprob" => self.markov_get_prob(channel_id),
                                _ => self.unrecognised_command(channel_id, message_id, subcmd),
                            }
                        } else {
                            "commands are:\n- `mark`\n- `unmark`\n- `getprob`\n- `setprob <value>`"
                                .into()
                        },
                        true,
                    ),
                    _ => BotCmd::SendText(
                        self.unrecognised_command(channel_id, message_id, cmd),
                        true,
                    ),
                }
            } else {
                BotCmd::SendText(SmolStr::new_inline("What do you want?"), true)
            }
        } else if message_reply_to
            .map(|message_id| self.has_insult_response(channel_id, message_id, message_content))
            .unwrap_or(false)
        {
            BotCmd::SendAttachment {
                name: SmolStr::new_inline("umad.jpg"),
                data: UMAD_JPG.to_vec(),
            }
        } else if self.data.user_id == message_author {
            if let Some(content) = self.try_insult(&channel_id, &message_id) {
                BotCmd::SendText(content, true)
            } else if let Some(content) = self.markov_try_gen_message(&channel_id, message_content)
            {
                BotCmd::SendText(content, false)
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
        if let Some(data) = self.data.insult_data.get(channel_id) {
            if let Some(msg_id) = data.message_id.as_deref() {
                if message_content.contains("fuck you") && message_id == msg_id {
                    return true;
                }
            }
        }
        false
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
            mlisten.chain.feed_str(message_content);
            if rand::thread_rng().gen_bool(mlisten.probability / 100.0) {
                let mut message = mlisten.chain.generate_str();
                message.truncate(250);
                return Some(message.into());
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

    pub fn process_poem_command(&self, keywords: SplitWhitespace) -> SmolStr {
        let poem_chain = &self.poem_chain;
        let keywords = keywords.map(str::to_lowercase).collect::<Vec<_>>();

        if keywords.is_empty() {
            choose_random_poem().into()
        } else if keywords.first().map(|s| s.as_str()) == Some("~gen") {
            let mut output = String::new();
            let some_tokens = poem_chain.generate();

            let mut rng = rand::thread_rng();
            let start_token = some_tokens
                .iter()
                .filter(|c| c.chars().next().unwrap().is_uppercase())
                .choose(&mut rng)
                .unwrap();
            let seperate_by = rng.gen_range(2..=3);
            for (index, sentence) in poem_chain
                .generate_str_from_token(start_token)
                .split(|c| matches!(c, '.' | '?' | '\n'))
                .filter(|sub| !sub.trim().is_empty())
                .take(7)
                .enumerate()
            {
                let sentence = sentence.trim();
                output.push_str(sentence);
                if !sentence.ends_with(',') {
                    output.push('.');
                }
                output.push('\n');
                if index % seperate_by == 0 {
                    output.push('\n');
                }
            }
            output.into()
        } else {
            let mut ranked = POEMS
                .split('-')
                .filter_map(|p| {
                    let mut has_keywords = 0;
                    for k in &keywords {
                        if p.to_lowercase().contains(k) {
                            has_keywords += 1;
                        }
                    }
                    if has_keywords > 0 {
                        Some((p, has_keywords))
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
    chain.feed_str(&POEMS.replace('-', ""));
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
            eprintln!("ERROR: {}", err);
        }
    };
    ($res:expr, |$val:ident| $do:expr) => {
        match $res {
            Ok($val) => $do,
            Err(err) => eprintln!("ERROR: {}", err),
        }
    };
}
