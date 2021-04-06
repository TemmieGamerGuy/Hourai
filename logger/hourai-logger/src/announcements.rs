use crate::Client;
use anyhow::Result;
use hourai::models::channel::GuildChannel;
use hourai::models::gateway::payload::{BanAdd, MemberRemove};
use hourai::models::id::*;
use hourai::models::user::User;
use hourai::models::voice::VoiceState;
use hourai::proto::guild_configs::*;
use hourai_redis::GuildConfig;

async fn get_config(client: &Client, guild_id: GuildId) -> Result<Option<AnnouncementConfig>> {
    let mut redis = client.redis.clone();
    Ok(GuildConfig::fetch(guild_id, &mut redis).await?)
}

pub async fn on_member_join(client: &Client, guild: GuildId, user: User) -> Result<()> {
    if let Some(config) = get_config(client, guild).await? {
        // TODO(james7132): let this be customizable.
        let msg = format!("<@{}> has joined the server.", user.id);
        broadcast(client, config.get_leaves(), msg);
    }
    Ok(())
}

pub async fn on_member_leave(client: &Client, evt: MemberRemove) -> Result<()> {
    if let Some(config) = get_config(&client, evt.guild_id).await? {
        // TODO(james7132): let this be customizable.
        let msg = format!("**{}** has left the server.", evt.user.name);
        broadcast(client, config.get_leaves(), msg);
    }
    Ok(())
}

pub async fn on_member_ban(client: &Client, evt: BanAdd) -> Result<()> {
    if let Some(config) = get_config(client, evt.guild_id).await? {
        // TODO(james7132): let this be customizable.
        let msg = format!("**{}** has been banned.", evt.user.name);
        broadcast(client, config.get_bans(), msg);
    }
    Ok(())
}

pub async fn on_voice_update(
    client: &Client,
    state: VoiceState,
    before: Option<ChannelId>,
) -> Result<()> {
    let guild = match state.guild_id {
        Some(guild) => guild,
        None => return Ok(()),
    };
    let mut redis = client.redis.clone();
    let before_channel = if let Some(id) = before {
        hourai_redis::CachedGuild::fetch_resource::<GuildChannel>(guild, id, &mut redis).await?
    } else {
        None
    };
    let after_channel = if let Some(id) = state.channel_id {
        hourai_redis::CachedGuild::fetch_resource::<GuildChannel>(guild, id, &mut redis).await?
    } else {
        None
    };
    if before_channel == after_channel {
        return Ok(());
    }
    let user = match state.member {
        Some(member) => member.user.name,
        None => return Ok(()),
    };
    if let Some(config) = get_config(client, guild).await? {
        // TODO(james7132): let this be customizable.
        let msg = match (before_channel, after_channel) {
            (Some(b), Some(a)) => {
                format!(
                    "**{}** moved from **{}** to **{}**.",
                    user,
                    b.get_name(),
                    a.get_name()
                )
            }
            (None, Some(ch)) => format!("**{}** joined **{}**.", user, ch.get_name()),
            (Some(ch), None) => format!("**{}** left **{}**.", user, ch.get_name()),
            (None, None) => return Ok(()),
        };

        broadcast(&client, config.get_voice(), msg);
    }

    Ok(())
}

pub fn broadcast(client: &Client, config: &AnnouncementTypeConfig, message: String) {
    async fn push(http: hourai::http::Client, channel: ChannelId, msg: String) -> Result<()> {
        http.create_message(channel).content(msg)?.await?;
        Ok(())
    }

    for channel in config.get_channel_ids() {
        let http = client.http_client.clone();
        let channel_id = ChannelId(*channel);
        let msg = message.clone();
        tokio::spawn(async move {
            if let Err(err) = push(http, channel_id, msg).await {
                tracing::error!("Error while making announcment in {}: {}", channel_id, err);
            }
        });
    }
}
