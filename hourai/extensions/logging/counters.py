from hourai import cogs
from hourai.bot import CounterKeys
from discord.ext import commands


class Counters(cogs.BaseCog):

    # TODO(james7132): Flush counters to a timeseries database

    def __init__(self, bot):
        self.bot = bot

    @commands.Cog.listener()
    async def on_message(self, message):
        key = CounterKeys.MESSAGES_RECIEVED
        self.bot.user_counters[message.author.id][key] += 1
        self.bot.channel_counters[message.channel.id][key] += 1
        if message.guild is not None:
            self.bot.guild_counters[message.guild.id][key] += 1

    @commands.Cog.listener()
    async def on_raw_message_delete(self, payload):
        key = CounterKeys.MESSAGES_DELETED
        self.bot.channel_counters[payload.channel_id][key] += 1
        if payload.guild_id is not None:
            self.bot.guild_counters[payload.guild_id][key] += 1
        if payload.cached_message is not None:
            self.bot.user_counters[payload.cached_message.author.id][key] += 1

    @commands.Cog.listener()
    async def on_raw_bulk_message_delete(self, payload):
        key = CounterKeys.MESSAGES_DELETED
        count = len(payload.message_ids)
        self.bot.channel_counters[payload.channel_id][key] += count
        if payload.guild_id is not None:
            self.bot.guild_counters[payload.guild_id][key] += count
        for cached_message in payload.cached_messages:
            self.bot.user_counters[cached_message.author.id][key] += 1

    @commands.Cog.listener()
    async def on_raw_message_edit(self, payload):
        key = CounterKeys.MESSAGES_EDITED
        # TODO(james7132): Update this when discord.py v1.3.x releases
        msg = payload.cached_message
        if msg is not None:
            self.bot.channel_counters[msg.channel.id][key] += 1
            self.bot.user_counters[msg.author.id][key] += 1
            if msg.guild is not None:
                self.bot.guild_counters[msg.guild.id][key] += 1

    @commands.Cog.listener()
    async def on_member_join(self, member):
        key = CounterKeys.MEMBERS_JOINED
        self.bot.guild_counters[member.guild.id][key] += 1

    @commands.Cog.listener()
    async def on_member_remove(self, member):
        key = CounterKeys.MEMBERS_LEFT
        self.bot.guild_counters[member.guild.id][key] += 1

    @commands.Cog.listener()
    async def on_member_ban(self, guild, user):
        key = CounterKeys.MEMBERS_BANNED
        self.bot.guild_counters[guild.id][key] += 1

    @commands.Cog.listener()
    async def on_member_unban(self, guild, user):
        key = CounterKeys.MEMBERS_UNBANNED
        self.bot.guild_counters[guild.id][key] += 1

    @commands.Cog.listener()
    async def on_verify_accept(self, member):
        key = CounterKeys.MEMBERS_VERIFIED
        self.bot.guild_counters[member.guild.id][key] += 1

    @commands.Cog.listener()
    async def on_verify_reject(self, member):
        key = CounterKeys.MEMBERS_REJECTED
        self.bot.guild_counters[member.guild.id][key] += 1
