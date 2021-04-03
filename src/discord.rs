use std::time::Duration;

use super::{perr, Bot, BotCmd};
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

const DATA_PATH: &str = "data";

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        ctx.set_activity(Activity::playing("G-go for it, yay. Mii, nipah~☆"))
            .await;

        let bot = self.clone();
        tokio::spawn(async move {
            loop {
                bot.save_to(DATA_PATH).expect("couldnt save");
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        let channel_id: SmolStr = new_message.channel_id.0.to_string().into();
        let message_id: SmolStr = new_message.id.to_string().into();
        let message_author: SmolStr = new_message.author.to_string().into();
        let message_content = &new_message.content;
        let message_reply_to: Option<SmolStr> = new_message
            .referenced_message
            .as_ref()
            .map(|msg| msg.id.to_string().into());

        let bot_cmd = self.process_args(
            &channel_id,
            &message_id,
            message_content,
            &message_author,
            message_reply_to.as_deref(),
        );
        match bot_cmd {
            BotCmd::SendAttachment { name, data } => {
                perr!(send_attach(&ctx, &new_message, data, name.into()).await)
            }
            BotCmd::SendText(content, is_reply) => {
                let content = serenity::utils::content_safe(
                    &ctx,
                    content.as_str(),
                    &ContentSafeOptions::default(),
                )
                .await;

                if is_reply {
                    perr!(new_message.reply(&ctx, content).await);
                } else if let Some(chan) = new_message
                    .guild(&ctx)
                    .await
                    .map(|mut g| g.channels.remove(&new_message.channel_id))
                    .flatten()
                {
                    perr!(chan.send_message(&ctx, |msg| msg.content(content)).await);
                }
            }
            BotCmd::DoNothing => {}
        }
    }
}

pub async fn main(rt_handle: tokio::runtime::Handle) {
    let bot = Bot::read_from(DATA_PATH).unwrap_or_else(|_| {
        let id = std::env::var("DISCORD_BOT_ID").expect("need bot id");
        Bot::new(SmolStr::new_inline("bern"), id.into())
    });
    let bot2 = bot.clone();

    let token = std::env::var("DISCORD_TOKEN").expect("need token");
    let mut client = Client::builder(token)
        .event_handler(bot)
        .await
        .expect("Error creating client");

    let cc = client.shard_manager.clone();
    ctrlc::set_handler(move || {
        rt_handle.block_on(async { cc.lock().await.shutdown_all().await });
        bot2.save_to(DATA_PATH).expect("couldnt save");
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
