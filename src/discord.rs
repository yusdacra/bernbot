use std::time::Duration;

use super::{perr, Bot, BotCmd};
use rand::Rng;
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

const DATA_PATH: &str = "data_discord";

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        ctx.set_activity(Activity::playing(&format!(
            "{} help | G-go for it, yay. Mii, nipah~☆",
            self.data.prefix
        )))
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

        let bot_cmd = self
            .process_args(
                &channel_id,
                &message_id,
                message_content,
                &message_author,
                message_reply_to.as_deref(),
            )
            .await;

        match bot_cmd {
            BotCmd::SendMessage {
                text,
                is_reply,
                attach,
            } => {
                let content = serenity::utils::content_safe(
                    &ctx,
                    text.as_str(),
                    &ContentSafeOptions::default(),
                )
                .await;

                if let Some(chan) = new_message
                    .guild(&ctx)
                    .await
                    .map(|mut g| g.channels.remove(&new_message.channel_id))
                    .flatten()
                {
                    let typing = ctx.http.start_typing(new_message.channel_id.0).unwrap();
                    let millis = rand::thread_rng().gen_range(400..=800);
                    tokio::time::sleep(Duration::from_millis(millis)).await;
                    perr!(
                        chan.send_message(&ctx, |msg| {
                            let m = msg.content(content).allowed_mentions(|c| c.empty_parse());
                            if is_reply {
                                m.reference_message(&new_message);
                            }
                            if let Some((name, data)) = attach {
                                m.add_file(AttachmentType::Bytes {
                                    data: data.into(),
                                    filename: name.into(),
                                });
                            }
                            m
                        })
                        .await
                    );
                    typing.stop();
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
