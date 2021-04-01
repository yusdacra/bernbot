use bernbot::{perr, Bot};
use serenity::{
    async_trait,
    client::{Client, Context, EventHandler},
    http::AttachmentType,
    model::{
        channel::Message,
        prelude::{Activity, Ready},
    },
    utils::ContentSafeOptions,
};
use std::{env, path::Path};

const DATA_PATH: &str = "data";

struct Handler {
    bot: Bot,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        ctx.set_activity(Activity::playing("G-go for it, yay. Mii, nipah~☆"))
            .await;
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        let channel_id = new_message.channel_id.0.to_string();
        let message_id = new_message.id.to_string();
        let bot_cmd = self
            .bot
            .process_args(&channel_id, &message_id, &new_message.content);
        if !matches!(bot_cmd, bernbot::BotCmd::DoNothing) {
            match bot_cmd {
                bernbot::BotCmd::ReplyWith(content) => {
                    perr!(new_message.reply(&ctx, content).await)
                }
                bernbot::BotCmd::SendAttachment { name, data } => {
                    perr!(send_attach(&ctx, &new_message, data, name).await)
                }
                bernbot::BotCmd::DoNothing => unreachable!(),
            }
        } else {
            if let Some(ref_msg) = new_message.referenced_message.as_ref() {
                if self.bot.has_insult_response(
                    &channel_id,
                    &ref_msg.id.to_string(),
                    &new_message.content,
                ) {
                    perr!(
                        send_attach(
                            &ctx,
                            &new_message,
                            bernbot::UMAD_JPG.to_vec(),
                            "umad.jpg".to_string()
                        )
                        .await
                    );
                }
            }

            if new_message.author.id != ctx.cache.current_user_id().await {
                if let Some(content) = self.bot.try_insult(&channel_id, &message_id) {
                    perr!(new_message.reply(&ctx, content).await);
                }

                if let Some(content) = self
                    .bot
                    .markov_try_gen_message(&channel_id, &new_message.content)
                {
                    let content = serenity::utils::content_safe(
                        &ctx,
                        &content,
                        &ContentSafeOptions::default(),
                    )
                    .await;
                    let cid = new_message.channel_id;
                    if let Some(chan) = new_message
                        .guild(&ctx)
                        .await
                        .map(|mut g| g.channels.remove(&cid))
                        .flatten()
                    {
                        perr!(chan.send_message(&ctx, |msg| msg.content(content)).await);
                    }
                }
            }
        }
        self.bot.save_to(Path::new(DATA_PATH)).unwrap();
    }
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let bot = Bot::read_from(Path::new(DATA_PATH)).unwrap_or_else(|_| Bot::new("bern"));
    let handler = Handler { bot };

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

async fn send_attach(
    ctx: &Context,
    msg: &Message,
    data: Vec<u8>,
    filename: String,
) -> serenity::Result<()> {
    let cid = msg.channel_id;
    if let Some(chan) = msg
        .guild(ctx)
        .await
        .map(|mut g| g.channels.remove(&cid))
        .flatten()
    {
        chan.send_message(ctx, |msg| {
            msg.add_file(AttachmentType::Bytes {
                data: data.into(),
                filename,
            })
        })
        .await?;
    }

    Ok(())
}
