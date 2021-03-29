use bernbot::perr;
use markov::Chain;
use rand::Rng;
use serenity::{
    async_trait,
    client::{Client, Context, EventHandler},
    futures::lock::Mutex,
    http::AttachmentType,
    model::{
        channel::Message,
        id::{ChannelId, GuildId},
        prelude::{Activity, Ready},
    },
};
use std::{borrow::Cow, collections::HashMap, convert::TryInto, env};

struct Handler {
    no_insult_count: Mutex<HashMap<GuildId, u8>>,
    last_insult_msg: Mutex<HashMap<ChannelId, Message>>,
    mchain: Mutex<HashMap<GuildId, Chain<String>>>,
    mlisten: Mutex<HashMap<GuildId, ChannelId>>,
    poem_chain: Chain<String>,
}

impl Default for Handler {
    fn default() -> Self {
        Self {
            no_insult_count: HashMap::new().into(),
            last_insult_msg: HashMap::new().into(),
            mchain: HashMap::new().into(),
            mlisten: HashMap::new().into(),
            poem_chain: {
                let mut chain = Chain::new();
                chain.feed_str(&bernbot::POEMS.split('-').collect::<String>());
                chain
            },
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        ctx.set_activity(Activity::playing("G-go for it, yay. Mii, nipah~☆"))
            .await;
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        let guild_id = new_message.guild_id.unwrap();
        if let Some(args) = new_message.content.strip_prefix(bernbot::PREFIX) {
            let mut args = args.split_whitespace();
            if let Some(cmd) = args.next() {
                match cmd {
                    "poem" => {
                        let output = bernbot::process_poem_command(args, &self.poem_chain);
                        perr!(new_message.reply(&ctx, output).await);
                    }
                    "fuckyou" => {
                        perr!(send_fuckyou(&ctx, &new_message).await);
                    }
                    "listentohere" => {
                        self.mlisten
                            .lock()
                            .await
                            .insert(guild_id, new_message.channel_id);
                        perr!(
                            new_message
                                .reply(
                                    &ctx,
                                    "This channel will be used to post \"random\" messages."
                                )
                                .await
                        );
                        tokio::fs::create_dir_all(format!("data/{}", guild_id.0))
                            .await
                            .unwrap();
                        tokio::fs::write(
                            format!("data/{}/mlisten", guild_id.0),
                            new_message.channel_id.0.to_be_bytes(),
                        )
                        .await
                        .unwrap()
                    }
                    _ => {
                        perr!(
                            new_message
                                .reply(&ctx, bernbot::unrecognised_command(cmd))
                                .await,
                            |msg| {
                                self.last_insult_msg
                                    .lock()
                                    .await
                                    .insert(new_message.channel_id, msg);
                            }
                        );
                    }
                }
            }
        } else {
            if let Some(ref_msg) = new_message.referenced_message.as_ref() {
                if let Some(last_ins_msg) =
                    self.last_insult_msg.lock().await.get(&ref_msg.channel_id)
                {
                    if new_message.content.contains("fuck you") && ref_msg.id == last_ins_msg.id {
                        perr!(send_fuckyou(&ctx, &new_message).await);
                    }
                }
            }

            if let Some(guild_id) = new_message.guild_id {
                let mut ins_map_lock = self.no_insult_count.lock().await;
                let no_insult_count = ins_map_lock.entry(guild_id).or_insert(1);
                if rand::thread_rng().gen_bool(0.2 * (*no_insult_count as f64) / 100.0) {
                    *no_insult_count = 1;
                    let res = new_message
                        .reply(&ctx, bernbot::choose_random_insult())
                        .await;
                    perr!(res, |msg| {
                        self.last_insult_msg
                            .lock()
                            .await
                            .insert(new_message.channel_id, msg);
                    });
                } else {
                    *no_insult_count = no_insult_count.saturating_add(1);
                }
            }

            if let Some(chan_id) = self.mlisten.lock().await.get(&guild_id) {
                if chan_id == &new_message.channel_id {
                    let mut chains_lock = self.mchain.lock().await;
                    let chain = chains_lock.entry(guild_id).or_insert_with(Chain::new);
                    chain.feed_str(&new_message.content);
                    if rand::thread_rng().gen_bool(5.0 / 100.0) {
                        if let Some(chan) = new_message
                            .guild(&ctx)
                            .await
                            .map(|mut g| g.channels.remove(chan_id))
                            .flatten()
                        {
                            let mut message = chain.generate_str();
                            message.truncate(250);
                            perr!(chan.send_message(&ctx, |msg| msg.content(message)).await);
                        }
                    }
                    chain
                        .save(format!("data/{}/mlisten_data", guild_id.0))
                        .unwrap();
                }
            }
        }
    }
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let mut mlisten_map = HashMap::new();
    let mut mchain_map = HashMap::new();

    std::fs::create_dir_all("data").unwrap();
    let dirs = std::fs::read_dir("data").unwrap().flatten();
    for dir in dirs {
        if dir.metadata().unwrap().is_dir() {
            let guild_id: u64 = dir.file_name().to_string_lossy().parse().unwrap();
            let mlisten = std::fs::read(dir.path().join("mlisten"))
                .ok()
                .map(|b| ChannelId(u64::from_be_bytes(b.try_into().unwrap())))
                .unwrap();
            let mlisten_data =
                Chain::load(dir.path().join("mlisten_data")).unwrap_or_else(|_| Chain::new());
            mlisten_map.insert(guild_id.into(), mlisten);
            mchain_map.insert(guild_id.into(), mlisten_data);
        }
    }

    let handler = Handler {
        mlisten: mlisten_map.into(),
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
