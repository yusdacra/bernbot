use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use crate::{perr, Bot, BotError, Handler};
use harmony::{
    api::{chat::event, exports::hrpc::async_trait, harmonytypes::Message},
    client::{
        api::{
            auth::Session,
            chat::{
                guild::{self, GetGuildListRequest},
                message::{self, SendMessage, SendMessageSelfBuilder},
                permissions::{self, QueryPermissions},
                profile::{self, ProfileUpdate},
                typing, EventSource, Typing,
            },
            harmonytypes::UserStatus,
            rest::{upload_extract_id, FileId},
        },
        error::{ClientError, ClientResult},
        Client,
    },
};
use rand::Rng;
use smol_str::SmolStr;
use tokio::{select, spawn};

const DATA_PATH: &str = "data_harmony";

struct HarmonyHandler<'a> {
    client: &'a Client,
    message: &'a Message,
    id: SmolStr,
    author: SmolStr,
    channel_id: SmolStr,
    guild_id: Option<SmolStr>,
    referenced_id: Option<SmolStr>,
}

#[async_trait]
impl<'a> Handler for HarmonyHandler<'a> {
    type Error = ClientError;

    async fn send_message(
        &self,
        text: &str,
        attach: Option<(&str, Vec<u8>)>,
        reply: bool,
    ) -> Result<SmolStr, BotError<Self::Error>> {
        let mut send_message =
            SendMessage::new(self.message.guild_id, self.message.channel_id, text.into());
        if let Some((name, data)) = attach {
            let attach_id =
                upload_extract_id(self.client, name.into(), "image/jpg".into(), data).await?;
            send_message = send_message.attachments(vec![FileId::Id(attach_id)]);
        }
        if reply {
            send_message = send_message.in_reply_to(self.message.message_id);
        }
        typing(
            self.client,
            Typing::new(self.message.guild_id, self.message.channel_id),
        )
        .await?;
        let millis = rand::thread_rng().gen_range(400..=800);
        tokio::time::sleep(Duration::from_millis(millis)).await;
        let new_message = message::send_message(self.client, send_message).await?;
        Ok(new_message.message_id.to_string().into())
    }

    async fn author_has_manage_perm(&self) -> Result<bool, BotError<Self::Error>> {
        let perm = permissions::query_has_permission(
            self.client,
            QueryPermissions::new(
                self.message.guild_id,
                self.message.channel_id,
                "manage".into(),
            ),
        )
        .await?;
        Ok(perm.ok)
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
        &self.message.content
    }

    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    fn guild_id(&self) -> Option<&str> {
        self.guild_id.as_deref()
    }
}

pub async fn main() -> ClientResult<()> {
    let session_token = std::env::var("HARMONY_TOKEN").expect("token");
    let user_id: SmolStr = std::env::var("HARMONY_ID").expect("user id").into();

    let server = std::env::var("HARMONY_SERVER")
        .unwrap_or_else(|_| "https://chat.harmonyapp.io:2289".to_string());

    let bot = Bot::read_from(DATA_PATH)
        .await
        .unwrap_or_else(|_| Bot::new(user_id.clone()));
    bot.start_autosave_task(DATA_PATH);
    let bot2 = bot.clone();

    let client = Client::new(
        server.parse().unwrap(),
        Some(Session {
            session_token,
            user_id: user_id.parse().unwrap(),
        }),
    )
    .await
    .unwrap();
    let client2 = client.clone();

    // Change our bots status to online and make sure its marked as a bot
    profile::profile_update(
        &client,
        ProfileUpdate::default()
            .new_status(UserStatus::OnlineUnspecified)
            .new_is_bot(true),
    )
    .await?;

    let guilds = guild::get_guild_list(&client, GetGuildListRequest {}).await?;

    // Subscribe to guild events
    let mut socket = client
        .subscribe_events(
            guilds
                .guilds
                .into_iter()
                .map(|c| EventSource::Guild(c.guild_id))
                .collect(),
        )
        .await?;

    let ctrlc = async move {
        tokio::signal::ctrl_c().await.unwrap();
        bot2.save_to(DATA_PATH).await.expect("couldnt save");
    };

    // Poll events
    let poll = async move {
        loop {
            if let Some(Ok(event::Event::SentMessage(sent_message))) = socket.get_event().await {
                if let Some(message) = sent_message.message {
                    let handler = HarmonyHandler {
                        id: message.message_id.to_string().into(),
                        author: message.author_id.to_string().into(),
                        channel_id: message.channel_id.to_string().into(),
                        guild_id: (message.guild_id != 0)
                            .then(|| message.guild_id.to_string().into()),
                        referenced_id: (message.in_reply_to != 0)
                            .then(|| message.in_reply_to.to_string().into()),
                        client: &client,
                        message: &message,
                    };
                    perr!(bot.process_args(&handler).await);
                }
            }
        }
    };
    select! {
        _ = spawn(poll) => {},
        _ = spawn(ctrlc) => {
            // Change our bots status back to offline
            let _ = profile::profile_update(
                &client2,
                ProfileUpdate::default().new_status(UserStatus::Offline),
            )
            .await;
        },
    }

    Ok(())
}
