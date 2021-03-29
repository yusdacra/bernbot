use std::str::SplitWhitespace;

use markov::Chain;
use rand::prelude::IteratorRandom;

pub const PREFIX: &str = "bern ";

pub const POEMS: &str = include_str!("poems.txt");
pub const INSULTS: &str = include_str!("insults.txt");
pub const UMAD_IMG: &[u8] = include_bytes!("umad.jpg");

pub fn choose_random_poem() -> &'static str {
    POEMS.split('-').choose(&mut rand::thread_rng()).unwrap()
}

pub fn choose_random_insult() -> &'static str {
    INSULTS.split('-').choose(&mut rand::thread_rng()).unwrap()
}

pub fn unrecognised_command(cmd: &str) -> String {
    format!("{}`{}` isn't even a command.", choose_random_insult(), cmd)
}

pub fn process_poem_command(keywords: SplitWhitespace, poem_chain: &Chain<String>) -> String {
    let keywords = keywords.map(str::to_lowercase).collect::<Vec<_>>();

    if keywords.is_empty() {
        choose_random_poem().to_string()
    } else if keywords.first().map(|s| s.as_str()) == Some("~gen") {
        let mut output = String::new();
        let some_tokens = poem_chain.generate();
        let start_token = some_tokens
            .iter()
            .filter(|c| c.chars().next().unwrap().is_uppercase())
            .choose(&mut rand::thread_rng())
            .unwrap();
        for (index, sentence) in poem_chain
            .generate_str_from_token(start_token)
            .split(|c| matches!(c, '.' | '?'))
            .take(7)
            .enumerate()
        {
            output.push_str(sentence.trim());
            output.push_str(".\n");
            if index % 3 == 0 {
                output.push('\n');
            }
        }
        output
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
            result.to_string()
        } else {
            "No poem with those words. Try again, maybe a miracle will occur.".to_string()
        }
    }
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
