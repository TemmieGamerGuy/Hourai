use anyhow::Result;
use crate::Client;
use hourai::{
    models::{RoleFlags, guild::{Member, Permissions}, id::*},
    proto::guild_configs::*,
};
use hourai_redis::GuildConfig;
use std::collections::HashMap;

async fn get_roles(
    client: &Client,
    guild_id: GuildId,
    user_id: UserId
) -> hourai_sql::Result<Vec<RoleId>> {
    hourai_sql::Member::fetch(guild_id, client.user_id)
        .fetch_one(&client.sql)
        .await
        .map(|member| member.role_ids().collect())
}

async fn get_role_flags(client: &Client, guild_id: GuildId) -> Result<HashMap<RoleId, RoleFlags>> {
    let config =
        GuildConfig::fetch_or_default::<RoleConfig>(guild_id, &mut client.redis.clone())
            .await?;
    Ok(config
        .get_settings()
        .iter()
        .map(|(k, v)| (RoleId(*k), RoleFlags::from_bits_truncate(v.get_flags())))
        .collect())
}

async fn get_verification_role(client: &Client, guild_id: GuildId) -> Result<Option<RoleId>> {
    let config =
        GuildConfig::fetch_or_default::<VerificationConfig>(guild_id, &mut client.redis.clone())
            .await?;

    if config.get_enabled() && config.has_role_id() {
        Ok(Some(RoleId(config.get_role_id())))
    } else {
        Ok(None)
    }
}

pub async fn on_member_join(client: &Client, member: &Member) -> Result<()> {
    let guild_id = member.guild_id;
    let user_id = member.user.id;

    let bot_roles = match get_roles(client, guild_id, client.user_id).await {
        Ok(roles) => roles,
        Err(hourai_sql::Error::RowNotFound) => return Ok(()),
        Err(err) => anyhow::bail!(err),
    };

    let perms = client.cache.guild_permissions(guild_id, client.user_id, bot_roles.iter().cloned());
    if !perms.contains(Permissions::MANAGE_ROLES) {
        return Ok(());
    }

    let user_roles = match get_roles(client, guild_id, user_id).await {
        Ok(roles) => roles,
        Err(hourai_sql::Error::RowNotFound) => return Ok(()),
        Err(err) => anyhow::bail!(err),
    };

    let max_role = client.cache.highest_role(bot_roles.iter().cloned());

    let flags = get_role_flags(client, guild_id).await?;
    let mut restorable: Vec<RoleId> =
        user_roles
            .iter()
            .filter_map(|id| client.cache.role(*id))
            .filter(|role| {
                let role_flags = flags.get(&role.id).cloned().unwrap_or(RoleFlags::empty());
                role.position < max_role && role_flags.contains(RoleFlags::RESTORABLE)
            })
            .map(|role| role.id)
            .collect();

    // Do not give out the verification role if it is enabled.
    if let Some(role) = get_verification_role(client, guild_id).await? {
        restorable.retain(|id| *id != role);
    }

    if restorable.is_empty() {
        return Ok(());
    }

    client.http_client
          .update_guild_member(guild_id, member.user.id)
          .roles(restorable)
          .await?;

    Ok(())
}
