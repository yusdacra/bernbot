use std::time::Duration;

use crate::{BotError, Handler, PRESENCE};

use super::{perr, Bot};
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

struct DiscordHandler<'a> {
    msg: &'a Message,
    ctx: &'a Context,
    id: SmolStr,
    author: SmolStr,
    channel_id: SmolStr,
    referenced_id: Option<SmolStr>,
    guild_id: Option<SmolStr>,
}

#[async_trait]
impl<'a> Handler for DiscordHandler<'a> {
    type Error = serenity::Error;

    async fn author_has_manage_perm(&self) -> Result<bool, BotError<Self::Error>> {
        Ok(self
            .msg
            .guild(self.ctx)
            .await
            .ok_or(serenity::Error::Model(
                serenity::model::ModelError::GuildNotFound,
            ))?
            .member(self.ctx, self.msg.author.id)
            .await?
            .permissions(self.ctx)
            .await?
            .manage_guild())
    }

    async fn send_message(
        &self,
        text: &str,
        attach: Option<(&str, Vec<u8>)>,
        reply: bool,
    ) -> Result<SmolStr, BotError<Self::Error>> {
        let content =
            serenity::utils::content_safe(self.ctx, text, &ContentSafeOptions::default()).await;

        let typing = self.ctx.http.start_typing(self.msg.channel_id.0).unwrap();
        let millis = rand::thread_rng().gen_range(400..=800);
        tokio::time::sleep(Duration::from_millis(millis)).await;
        let msg = self
            .msg
            .channel_id
            .send_message(self.ctx, |msg| {
                let m = msg.content(content).allowed_mentions(|c| c.empty_parse());
                if reply {
                    m.reference_message(self.msg);
                }
                if let Some((name, data)) = attach {
                    m.add_file(AttachmentType::Bytes {
                        data: data.into(),
                        filename: name.into(),
                    });
                }
                m
            })
            .await?;
        typing.stop();
        Ok(msg.id.0.to_string().into())
    }

    fn referenced_id(&self) -> Option<&str> {
        self.referenced_id.as_deref()
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn author(&self) -> &str {
        &self.author
    }

    fn content(&self) -> &str {
        &self.msg.content
    }

    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    fn guild_id(&self) -> Option<&str> {
        self.guild_id.as_deref()
    }
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        ctx.set_activity(Activity::playing(PRESENCE)).await;

        self.start_autosave_task(DATA_PATH);
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        let channel_id: SmolStr = new_message.channel_id.0.to_string().into();
        let guild_id: Option<SmolStr> = new_message.guild_id.map(|i| i.as_u64().to_string().into());
        let id: SmolStr = new_message.id.0.to_string().into();
        let author: SmolStr = new_message.author.id.0.to_string().into();
        let referenced_id: Option<SmolStr> = new_message
            .referenced_message
            .as_ref()
            .map(|msg| msg.id.0.to_string().into());

        let handler = DiscordHandler {
            msg: &new_message,
            ctx: &ctx,
            channel_id,
            id,
            referenced_id,
            author,
            guild_id,
        };

        perr!(self.process_args(&handler,).await);
    }
}

pub async fn main(rt_handle: tokio::runtime::Handle) {
    let token = std::env::var("DISCORD_TOKEN").expect("need token");

    let bot = Bot::read_from(DATA_PATH).unwrap_or_else(|_| {
        let user_id = rt_handle
            .block_on(serenity::http::client::Http::new_with_token(&token).get_current_user())
            .expect("expect user")
            .id
            .0
            .to_string();
        Bot::new(user_id.into())
    });
    let bot2 = bot.clone();

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
