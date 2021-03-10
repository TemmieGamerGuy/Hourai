mod message_logging;

use anyhow::Result;
use tracing::{info, error, debug};
use hourai::{
    init, config,
    cache::{InMemoryCache, ResourceType},
    models::{
        id::*,
        channel::Message,
        gateway::payload::*,
        guild::Permissions,
        guild::member::Member,
        user::User,
    },
    gateway::{
        Intents, Event, EventType, EventTypeFlags,
        cluster::*,
    }
};
use hourai_sql::*;
use hourai_redis::*;
use futures::stream::StreamExt;
use core::time::Duration;

const BOT_INTENTS: Intents = Intents::from_bits_truncate(
    Intents::GUILDS.bits() |
    Intents::GUILD_BANS.bits() |
    Intents::GUILD_MESSAGES.bits() |
    Intents::GUILD_MEMBERS.bits() |
    Intents::GUILD_PRESENCES.bits());

const BOT_EVENTS : EventTypeFlags =
    EventTypeFlags::from_bits_truncate(
        EventTypeFlags::READY.bits() |
        EventTypeFlags::BAN_ADD.bits() |
        EventTypeFlags::BAN_REMOVE.bits() |
        EventTypeFlags::MEMBER_ADD.bits() |
        EventTypeFlags::MEMBER_CHUNK.bits() |
        EventTypeFlags::MEMBER_REMOVE.bits() |
        EventTypeFlags::MEMBER_UPDATE.bits() |
        EventTypeFlags::MESSAGE_CREATE.bits() |
        EventTypeFlags::MESSAGE_UPDATE.bits() |
        EventTypeFlags::MESSAGE_DELETE.bits() |
        EventTypeFlags::MESSAGE_DELETE_BULK.bits() |
        EventTypeFlags::GUILD_CREATE.bits() |
        EventTypeFlags::GUILD_UPDATE.bits() |
        EventTypeFlags::GUILD_DELETE.bits() |
        EventTypeFlags::PRESENCE_UPDATE.bits() |
        EventTypeFlags::ROLE_CREATE.bits() |
        EventTypeFlags::ROLE_UPDATE.bits() |
        EventTypeFlags::ROLE_DELETE.bits());

const CACHED_RESOURCES: ResourceType =
    ResourceType::from_bits_truncate(
        ResourceType::ROLE.bits() |
        ResourceType::GUILD.bits() |
        ResourceType::PRESENCE.bits() |
        ResourceType::USER_CURRENT.bits());

#[tokio::main]
async fn main() {
    let config = config::load_config(config::get_config_path().as_ref());

    init::init(&config);
    let http_client = init::http_client(&config);
    let sql = hourai_sql::init(&config).await;
    let redis = hourai_redis::init(&config).await;
    let cache = InMemoryCache::builder().resource_types(CACHED_RESOURCES).build();
    let gateway = Cluster::builder(&config.discord.bot_token, BOT_INTENTS)
            .shard_scheme(ShardScheme::Auto)
            .http_client(http_client.clone())
            .build()
            .await
            .expect("Failed to connect to the Discord gateway");

    let client = Client {
        http_client,
        gateway: gateway.clone(),
        cache: cache.clone(),
        sql,
        redis: redis.clone(),
    };

    info!("Starting gateway...");
    gateway.up().await;
    info!("Client started.");

    // Setup background tasks
    tokio::spawn(client.clone().log_bans());
    tokio::spawn(flush_online(cache.clone(), redis.clone()));

    let mut events = gateway.some_events(BOT_EVENTS);
    while let Some((shard_id, evt)) = events.next().await {
        cache.update(&evt);
        if evt.kind() != EventType::PresenceUpdate {
            tokio::spawn(client.clone().consume_event(shard_id, evt));
        }
    }

    info!("Shutting down gateway...");
    gateway.down();
    info!("Client stopped.");
}

async fn flush_online(cache: InMemoryCache, mut redis: RedisPool) {
    loop {
        let mut pipeline = OnlineStatus::new();
        for guild_id in cache.guilds() {
            let presences = match cache.guild_online(guild_id) {
                Some(p) => p,
                _ => continue,
            };
            pipeline.set_online(guild_id, presences);
        }
        let result = pipeline.build().query_async::<RedisPool, ()>(&mut redis).await;
        if let Err(err) = result {
            error!("Error while flushing statuses: {:?}", err);
        }
        tokio::time::sleep(Duration::from_secs(60u64)).await;
    }
}

#[derive(Clone)]
struct Client {
    pub http_client: hourai::http::Client,
    pub gateway: Cluster,
    pub cache: InMemoryCache,
    pub sql: SqlPool,
    pub redis: RedisPool,
}

impl Client {

    fn user_id(&self) -> UserId {
        self.cache.current_user().unwrap().id
    }

    async fn log_bans(self) {
        loop {
            info!("Refreshing bans...");
            for guild_id in self.cache.guilds() {
                if let Err(err) = self.refresh_bans(guild_id).await {
                    error!("Error while logging bans: {:?}", err);
                }
            }
            tokio::time::sleep(Duration::from_secs(180u64)).await;
        }
    }

    #[inline(always)]
    pub fn total_shards(&self) -> u64 {
        let shards = self.gateway.config().shard_config().shard()[1];
        assert!(shards > 0, "Bot somehow has a total of zero shards.");
        shards
    }

    /// Gets the shard ID for a guild.
    #[inline(always)]
    pub fn shard_id(&self, guild_id: GuildId) -> u64 {
        (guild_id.0 >> 22) % self.total_shards()
    }

    pub async fn chunk_guild(&self, guild_id: GuildId) -> Result<()> {
        debug!("Chunking guild: {}", guild_id);
        let request = RequestGuildMembers::builder(guild_id)
            .presences(true)
            .query(String::new(), None);
        self.gateway.command(self.shard_id(guild_id), &request).await?;
        Ok(())
    }

    pub async fn fetch_guild_permissions(
        &self,
        guild_id: GuildId,
        user_id: UserId
    ) -> Result<Permissions> {
        let local_member = hourai_sql::Member::fetch(guild_id, user_id)
                                .fetch_one(&self.sql)
                                .await;
        if let Ok(member) = local_member {
            Ok(self.cache.guild_permissions(guild_id, user_id, member.role_ids()))
        } else {
            let roles = self.http_client
                .guild_member(guild_id, user_id)
                .await?
                .into_iter()
                .flat_map(|m| m.roles);
            Ok(self.cache.guild_permissions(guild_id, user_id, roles))
        }
    }

    async fn consume_event(self, shard_id: u64, event: Event) -> () {
        let kind = event.kind();
        let result = match event {
            Event::Ready(_) => self.on_shard_ready(shard_id).await,
            Event::BanAdd(evt) => self.on_ban_add(evt).await,
            Event::BanRemove(evt) => self.on_ban_remove(evt).await,
            Event::GuildCreate(evt) => self.on_guild_create(*evt).await,
            Event::GuildUpdate(_) => Ok(()),
            Event::GuildDelete(evt) => {
                if !evt.unavailable {
                    self.on_guild_leave(*evt).await
                } else {
                    Ok(())
                }
            },
            Event::MemberAdd(evt) => self.on_member_add(*evt).await,
            Event::MemberChunk(evt) => self.on_member_chunk(evt).await,
            Event::MemberRemove(evt) => self.on_member_remove(evt).await,
            Event::MemberUpdate(evt) => self.on_member_update(*evt).await,
            Event::MessageCreate(evt) => self.on_message_create(evt.0).await,
            Event::MessageUpdate(evt) => self.on_message_update(*evt).await,
            Event::MessageDelete(evt) => self.on_message_delete(evt).await,
            Event::MessageDeleteBulk(evt) => self.on_message_bulk_delete(evt).await,
            Event::RoleCreate(_) => Ok(()),
            Event::RoleUpdate(_) => Ok(()),
            Event::RoleDelete(evt) => self.on_role_delete(evt).await,
            _ => {
                error!("Unexpected event type: {:?}", event);
                Ok(())
            },
        };

        if let Err(err) = result {
            error!("Error while running event with {:?}: {:?}", kind, err);
        }
    }

    async fn on_shard_ready(self, shard_id: u64) -> Result<()> {
        Ban::clear_shard(shard_id, self.total_shards())
            .execute(&self.sql)
            .await?;
        Ok(())
    }

    async fn on_ban_add(self, evt: BanAdd) -> Result<()> {
        self.log_users(vec![evt.user.clone()]).await?;

        let perms = self.fetch_guild_permissions(evt.guild_id, self.user_id()).await?;
        if perms.contains(Permissions::BAN_MEMBERS) {
            if let Some(ban) = self.http_client.ban(evt.guild_id, evt.user.id).await? {
                Ban::from(evt.guild_id, ban)
                    .insert()
                    .execute(&self.sql)
                    .await?;
            }
        }

        Ok(())
    }

    async fn on_ban_remove(self, evt: BanRemove) -> Result<()> {
        futures::join!(
            self.log_users(vec![evt.user.clone()]),
            Ban::clear_ban(evt.guild_id, evt.user.id).execute(&self.sql)
        );
        Ok(())
    }

    async fn on_member_add(self, evt: MemberAdd) -> Result<()> {
        self.log_members(&vec![evt.0]).await?;
        Ok(())
    }

    async fn on_member_chunk(self, evt: MemberChunk) -> Result<()> {
        self.log_members(&evt.members).await?;
        Ok(())
    }

    async fn on_member_remove(self, evt: MemberRemove) -> Result<()> {
        self.log_users(vec![evt.user]).await?;
        Ok(())
    }

    async fn on_message_create(mut self, evt: Message) -> Result<()> {
        if !evt.author.bot {
            CachedMessage::new(evt).flush().query_async(&mut self.redis).await?;
        }
        Ok(())
    }

    async fn on_message_update(mut self, evt: MessageUpdate) -> Result<()> {
        // TODO(james7132): Properly implement this
        let cached = CachedMessage::fetch(evt.channel_id, evt.id, &mut self.redis)
                                       .await?;
        if let Some(mut msg) = cached {
            if let Some(content) = evt.content {
                let before = msg.clone();
                msg.set_content(content);
                tokio::spawn(message_logging::on_message_update(self.clone(),
                             before.clone(), msg));
                CachedMessage::new(before).flush().query_async(&mut self.redis).await?;
            }
        }

        Ok(())
    }

    async fn on_message_delete(mut self, evt: MessageDelete) -> Result<()> {
        message_logging::on_message_delete(&mut self, &evt).await?;
        CachedMessage::delete(evt.channel_id, evt.id).query_async(&mut self.redis).await?;
        Ok(())
    }

    async fn on_message_bulk_delete(mut self, evt: MessageDeleteBulk) -> Result<()> {
        tokio::spawn(message_logging::on_message_bulk_delete(self.clone(), evt.clone()));
        CachedMessage::bulk_delete(evt.channel_id, evt.ids)
                          .query_async(&mut self.redis).await?;
        Ok(())
    }

    async fn on_guild_create(self, evt: GuildCreate) -> Result<()> {
        let guild = evt.0;

        if guild.unavailable {
            info!("Joined Guild: {}", guild.id);
        } else {
            info!("Guild Available: {}", guild.id);
        }

        self.chunk_guild(guild.id).await?;

        Ok(())
    }

    async fn on_guild_leave(self, evt: GuildDelete) -> Result<()> {
        info!("Left guild {}", evt.id);
        futures::join!(
            hourai_sql::Member::clear_guild(evt.id).execute(&self.sql),
            Ban::clear_guild(evt.id).execute(&self.sql)
        );
        Ok(())
    }

    async fn on_role_delete(self, evt: RoleDelete) -> Result<()> {
        let res = hourai_sql::Member::clear_role(evt.guild_id, evt.role_id)
            .execute(&self.sql)
            .await;
        self.refresh_bans(evt.guild_id).await?;
        res?;
        Ok(())
    }

    async fn on_member_update(self, evt: MemberUpdate) -> Result<()> {
        if evt.user.bot {
            return Ok(());
        }

        hourai_sql::Member {
            guild_id: evt.guild_id.0 as i64,
            user_id: evt.user.id.0 as i64,
            role_ids: evt.roles.iter().map(|id| id.0 as i64).collect(),
            nickname: evt.nick
        }.insert()
         .execute(&self.sql)
         .await?;
        Username::new(&evt.user)
            .insert()
            .execute(&self.sql)
            .await?;
        Ok(())
    }

    async fn log_users(&self, users: Vec<User>) -> Result<()> {
        let usernames = users.iter().map(|u| Username::new(u)).collect();
        Username::bulk_insert(usernames).execute(&self.sql).await?;
        Ok(())
    }

    async fn log_members(&self, members: &Vec<Member>) -> Result<()> {
        let usernames = members.iter().map(|m| Username::new(&m.user)).collect();

        Username::bulk_insert(usernames).execute(&self.sql).await?;
        let mut txn = self.sql.begin().await?;
        for member in members {
            hourai_sql::Member::from(&member)
                .insert()
                .execute(&mut txn)
                .await?;
        }
        txn.commit().await?;
        Ok(())
    }

    async fn refresh_bans(&self, guild_id: GuildId) -> Result<()> {
        let perms = self.fetch_guild_permissions(guild_id, self.user_id()).await?;

        if perms.contains(Permissions::BAN_MEMBERS) {
            debug!("Fetching bans from guild {}", guild_id);
            let bans: Vec<Ban> = self.http_client.bans(guild_id).await?
                                         .into_iter().map(|b| Ban::from(guild_id, b))
                                         .collect();
            debug!("Fetched {} bans from guild {}", bans.len(), guild_id);
            let mut txn = self.sql.begin().await?;
            Ban::clear_guild(guild_id).execute(&mut txn).await?;
            Ban::bulk_insert(bans).execute(&mut txn).await?;
            txn.commit().await?;
        } else {
            debug!("Cleared bans from guild {}", guild_id);
            Ban::clear_guild(guild_id).execute(&self.sql).await?;
        }
        Ok(())
    }


}