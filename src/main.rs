use bernbot::perr;
use markov::Chain;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::{
    async_trait,
    client::{Client, Context, EventHandler},
    futures::lock::Mutex,
    http::AttachmentType,
    model::{
        channel::Message,
        prelude::{Activity, Ready},
    },
    utils::ContentSafeOptions,
};
use std::{borrow::Cow, collections::HashMap, env, num::NonZeroU8};

#[derive(Debug, Deserialize, Serialize)]
struct MListen {
    chan_id: u64,
    probability: f64,
    chain: Chain<String>,
}

impl MListen {
    async fn save(&self, guild_id: u64) {
        tokio::fs::write(
            format!("data/{}", guild_id),
            bincode::serialize(self).unwrap(),
        )
        .await
        .unwrap();
    }
}

impl Default for MListen {
    fn default() -> Self {
        Self {
            chan_id: 0,
            probability: 5.0,
            chain: Chain::new(),
        }
    }
}

struct Handler {
    insult_data: Mutex<HashMap<u64, (NonZeroU8, Option<Message>)>>,
    mchain: Mutex<HashMap<u64, MListen>>,
    poem_chain: Chain<String>,
}

impl Default for Handler {
    fn default() -> Self {
        Self {
            insult_data: HashMap::new().into(),
            mchain: HashMap::new().into(),
            poem_chain: {
                let mut chain = Chain::new();
                chain.feed_str(&bernbot::POEMS.split('-').collect::<String>());
                chain
            },
        }
    }
}

impl Handler {
    async fn unrecognised_command(&self, ctx: &Context, msg: &Message, cmd: &str, channel_id: u64) {
        perr!(
            msg.reply(ctx, bernbot::unrecognised_command(cmd)).await,
            |msg| {
                self.insult_data
                    .lock()
                    .await
                    .insert(channel_id, (NonZeroU8::new(1).unwrap(), Some(msg)));
            }
        )
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        ctx.set_activity(Activity::playing("G-go for it, yay. Mii, nipah~☆"))
            .await;
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        let guild_id = new_message.guild_id.unwrap().0;
        let channel_id = new_message.channel_id.0;
        if let Some(args) = new_message.content.strip_prefix(bernbot::PREFIX) {
            let mut args = args.split_whitespace();
            if let Some(cmd) = args.next() {
                match cmd {
                    "poem" => perr!(
                        new_message
                            .reply(&ctx, bernbot::process_poem_command(args, &self.poem_chain))
                            .await
                    ),
                    "fuckyou" => perr!(send_fuckyou(&ctx, &new_message).await),
                    "listen" => {
                        const SUBCMDS: [&str; 3] = ["here", "setprob", "getprob"];

                        if let Some(subcmd) = args.next() {
                            if SUBCMDS.contains(&subcmd) {
                                let msg = match subcmd {
                                    "here" => {
                                        let mut lock = self.mchain.lock().await;
                                        let mut mlisten = lock.entry(guild_id).or_default();
                                        mlisten.chan_id = channel_id;
                                        mlisten.save(guild_id).await;
                                        "This channel will be used to post \"random\" messages."
                                            .to_string()
                                    }
                                    "setprob" => {
                                        let prob = args
                                            .next()
                                            .map_or(5.0, |c| c.parse::<f64>().unwrap_or(5.0))
                                            .min(100.0)
                                            .max(0.0);
                                        if let Some(mlisten) =
                                            self.mchain.lock().await.get_mut(&guild_id)
                                        {
                                            mlisten.probability = prob;
                                            mlisten.save(guild_id).await;
                                            format!("Set probability to {}%", prob)
                                        } else {
                                            "First set a channel to listen in, dumb human."
                                                .to_string()
                                        }
                                    }
                                    "getprob" => {
                                        if let Some(mlisten) =
                                            self.mchain.lock().await.get(&guild_id)
                                        {
                                            format!("Probability is {}%", mlisten.probability)
                                        } else {
                                            "First set a channel to listen in, dumb human."
                                                .to_string()
                                        }
                                    }
                                    _ => unreachable!("literally how"),
                                };
                                perr!(new_message.reply(&ctx, msg,).await);
                            } else {
                                self.unrecognised_command(&ctx, &new_message, subcmd, channel_id)
                                    .await
                            }
                        }
                    }
                    _ => {
                        self.unrecognised_command(&ctx, &new_message, cmd, channel_id)
                            .await
                    }
                }
            }
        } else {
            let mut lock = self.insult_data.lock().await;

            if let Some(ref_msg) = new_message.referenced_message.as_ref() {
                if let Some((_, Some(last_ins_msg))) = lock.get(&ref_msg.channel_id.0) {
                    if new_message.content.contains("fuck you") && ref_msg.id == last_ins_msg.id {
                        perr!(send_fuckyou(&ctx, &new_message).await);
                    }
                }
            }

            let (no_insult_count, last_ins_msg) = lock
                .entry(guild_id)
                .or_insert((NonZeroU8::new(1).unwrap(), None));
            if rand::thread_rng().gen_bool(0.05 * (no_insult_count.get() as f64) / 100.0) {
                *no_insult_count = NonZeroU8::new(1).unwrap();
                let res = new_message
                    .reply(&ctx, bernbot::choose_random_insult())
                    .await;
                perr!(res, |msg| {
                    *last_ins_msg = Some(msg);
                });
            } else {
                *no_insult_count = NonZeroU8::new(no_insult_count.get().saturating_add(1)).unwrap();
            }
            drop(lock);

            if new_message.author.id != ctx.cache.current_user_id().await {
                if let Some(mlisten) = self.mchain.lock().await.get_mut(&guild_id) {
                    if mlisten.chan_id == channel_id {
                        mlisten.chain.feed_str(&new_message.content);
                        if rand::thread_rng().gen_bool(mlisten.probability / 100.0) {
                            if let Some(chan) = new_message
                                .guild(&ctx)
                                .await
                                .map(|mut g| g.channels.remove(&mlisten.chan_id.into()))
                                .flatten()
                            {
                                let mut message = mlisten.chain.generate_str();
                                message.truncate(250);
                                let message = serenity::utils::content_safe(
                                    &ctx,
                                    &message,
                                    &ContentSafeOptions::default(),
                                )
                                .await;
                                perr!(chan.send_message(&ctx, |msg| msg.content(message)).await);
                            }
                        }
                        mlisten.save(guild_id).await;
                    }
                }
            }
        }
    }
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let mut mchain_map = HashMap::new();

    std::fs::create_dir_all("data").unwrap();
    let dirs = std::fs::read_dir("data").unwrap().flatten();
    for dir in dirs {
        let guild_id: u64 = dir.file_name().to_string_lossy().parse().unwrap();
        let mlisten: MListen = bincode::deserialize(&std::fs::read(dir.path()).unwrap()).unwrap();
        mchain_map.insert(guild_id, mlisten);
    }

    let handler = Handler {
        mchain: mchain_map.into(),
        ..Handler::default()
    };

    let token = env::var("DISCORD_TOKEN").expect("token");
    let mut client = runtime
        .block_on(Client::builder(token).event_handler(handler))
        .expect("Error creating client");

    let cc = client.shard_manager.clone();
    let rt_handle = runtime.handle().clone();
    ctrlc::set_handler(move || {
        rt_handle.block_on(async { cc.lock().await.shutdown_all().await });
        std::process::exit(0);
    })
    .expect("couldnt set ctrlc handler");

    perr!(runtime.block_on(client.start()));

    runtime.shutdown_background();
}

async fn send_fuckyou(ctx: &Context, msg: &Message) -> serenity::Result<()> {
    let cid = msg.channel_id;
    if let Some(chan) = msg
        .guild(ctx)
        .await
        .map(|mut g| g.channels.remove(&cid))
        .flatten()
    {
        chan.send_message(ctx, |msg| {
            msg.add_file(AttachmentType::Bytes {
                data: Cow::Borrowed(bernbot::UMAD_IMG),
                filename: "umad.jpg".into(),
            })
        })
        .await?;
    }

    Ok(())
}
