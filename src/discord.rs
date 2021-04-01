use super::{perr, Bot, BotCmd, UMAD_JPG};
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
use smol_str::SmolStr;
use std::{env, path::Path};

const DATA_PATH: &str = "data";

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        ctx.set_activity(Activity::playing("G-go for it, yay. Mii, nipah~☆"))
            .await;
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        let channel_id: SmolStr = new_message.channel_id.0.to_string().into();
        let message_id: SmolStr = new_message.id.to_string().into();

        let bot_cmd = self.process_args(&channel_id, &message_id, &new_message.content);
        if !matches!(bot_cmd, BotCmd::DoNothing) {
            match bot_cmd {
                BotCmd::ReplyWith(content) => {
                    perr!(new_message.reply(&ctx, content).await)
                }
                BotCmd::SendAttachment { name, data } => {
                    perr!(send_attach(&ctx, &new_message, data, name.into()).await)
                }
                BotCmd::DoNothing => unreachable!(),
            }
        } else {
            if let Some(ref_msg) = new_message.referenced_message.as_ref() {
                if self.has_insult_response(
                    &channel_id,
                    &ref_msg.id.to_string(),
                    &new_message.content,
                ) {
                    perr!(
                        send_attach(
                            &ctx,
                            &new_message,
                            UMAD_JPG.to_vec(),
                            "umad.jpg".to_string()
                        )
                        .await
                    );
                }
            }

            if new_message.author.id != ctx.cache.current_user_id().await {
                if let Some(content) = self.try_insult(&channel_id, &message_id) {
                    perr!(new_message.reply(&ctx, content).await);
                }

                if let Some(content) =
                    self.markov_try_gen_message(&channel_id, &new_message.content)
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
        perr!(self.save_to(Path::new(DATA_PATH)));
    }
}

pub async fn discord_main(rt_handle: tokio::runtime::Handle) {
    let bot = Bot::read_from(Path::new(DATA_PATH)).unwrap_or_else(|_| Bot::new("bern"));

    let token = env::var("DISCORD_TOKEN").expect("token");
    let mut client = Client::builder(token)
        .event_handler(bot)
        .await
        .expect("Error creating client");

    let cc = client.shard_manager.clone();
    ctrlc::set_handler(move || {
        rt_handle.block_on(async { cc.lock().await.shutdown_all().await });
        std::process::exit(0);
    })
    .expect("couldnt set ctrlc handler");

    perr!(client.start().await);
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
