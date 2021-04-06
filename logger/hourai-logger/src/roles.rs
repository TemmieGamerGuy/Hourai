use crate::Client;
use anyhow::Result;
use hourai::{
    models::{
        guild::{Member, Permissions, Role},
        id::*,
        RoleFlags,
    },
    proto::guild_configs::*,
};
use hourai_redis::GuildConfig;
use std::collections::HashMap;

async fn get_roles(
    client: &Client,
    guild_id: GuildId,
    user_id: UserId,
) -> hourai_sql::Result<Vec<RoleId>> {
    hourai_sql::Member::fetch(guild_id, user_id)
        .fetch_one(&client.sql)
        .await
        .map(|member| member.role_ids().collect())
}

async fn get_role_flags(client: &Client, guild_id: GuildId) -> Result<HashMap<u64, RoleFlags>> {
    let config =
        GuildConfig::fetch_or_default::<RoleConfig>(guild_id, &mut client.redis.clone()).await?;
    Ok(config
        .get_settings()
        .iter()
        .map(|(k, v)| (*k, RoleFlags::from_bits_truncate(v.get_flags())))
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

    let mut redis = client.redis.clone();
    let perms = hourai_redis::CachedGuild::guild_permissions(
        guild_id,
        client.user_id,
        bot_roles.iter().cloned(),
        &mut redis,
    )
    .await?;
    if !perms.contains(Permissions::MANAGE_ROLES) {
        return Ok(());
    }

    let user_roles = match get_roles(client, guild_id, user_id).await {
        Ok(roles) => {
            hourai_redis::CachedGuild::fetch_resources::<Role>(guild_id, &roles, &mut redis).await?
        }
        Err(hourai_sql::Error::RowNotFound) => return Ok(()),
        Err(err) => anyhow::bail!(err),
    };

    let max_role =
        hourai_redis::CachedGuild::highest_role(guild_id, &bot_roles, &mut redis).await?;

    let flags = get_role_flags(client, guild_id).await?;
    let mut restorable: Vec<RoleId> = user_roles
        .iter()
        .filter(|role| {
            let role_flags = flags
                .get(&role.get_role_id())
                .cloned()
                .unwrap_or_else(RoleFlags::empty);
            role.get_position() < max_role && role_flags.contains(RoleFlags::RESTORABLE)
        })
        .map(|role| RoleId(role.get_role_id()))
        .collect();

    // Do not give out the verification role if it is enabled.
    if let Some(role) = get_verification_role(client, guild_id).await? {
        restorable.retain(|id| *id != role);
    }

    if restorable.is_empty() {
        return Ok(());
    }

    client
        .http_client
        .update_guild_member(guild_id, member.user.id)
        .roles(restorable)
        .await?;

    Ok(())
}
