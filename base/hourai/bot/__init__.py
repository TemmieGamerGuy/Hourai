import aiohttp
import asyncio
import collections
import discord
import enum
import time
import logging
import pkgutil
import sys
from discord.ext import commands
from hourai import config
from hourai.db import storage
from hourai.utils import fake
from . import actions, extensions
from .context import HouraiContext
from .guild import HouraiGuild


log = logging.getLogger(__name__)


class HouraiConnectionState(discord.state.AutoShardedConnectionState):

    def __init__(self, *args, **kwargs):
        self.storage = kwargs.pop('storage')
        super().__init__(*args, **kwargs)

    def _add_guild_from_data(self, guild):
        guild = HouraiGuild(data=guild, state=self)
        self._add_guild(guild)
        return guild


class Hourai(commands.AutoShardedBot):

    def __init__(self, *args, **kwargs):
        self.logger = log
        try:
            self.config = kwargs['config']
        except KeyError:
            raise ValueError(
                '"config" must be specified when initialzing Hourai.')
        self.storage = kwargs.get('storage') or storage.Storage(self.config)

        kwargs = {
            'max_messages': None,
            'description': self.config.description,
            'command_prefix': self.config.command_prefix,
            'activity': discord.Game(self.config.activity),
            'help_command': HouraiHelpCommand(),
            'fetch_offline_members': False,
            'member_cache_flags': discord.MemberCacheFlags(
                online=False,
                joined=True,
                voice=False),
            'allowed_mentions': discord.AllowedMentions(
                everyone=False,
                users=True,
                roles=False),
            'intents': discord.Intents(
                bans=True,
                guilds=True,
                invites=True,
                members=True,
                messages=True,
                presences=False,
                reactions=True,
                typing=False,
                voice_states=True,
                emojis=False,
                integrations=False,
                webhooks=False),
            **kwargs
        }

        super().__init__(*args, **kwargs)
        self.http_session = aiohttp.ClientSession(loop=self.loop)
        self.action_manager = actions.ActionManager(self)

    def _get_state(self, **options):
        return HouraiConnectionState(
                storage=self.storage, dispatch=self.dispatch,
                handlers=self._handlers, syncer=self._syncer,
                hooks=self._hooks, http=self.http, loop=self.loop, **options)

    def create_storage_session(self):
        return self.storage.create_session()

    async def get_member_async(self, guild: discord.Guild, user_id: int) \
            -> discord.Member:
        member = guild.get_member(user_id)
        if member:
            return member
        elif self.get_shard(guild.shard_id).is_ws_ratelimited():
            # If we're being rate limited on the WS, then fall back to using
            # the HTTP API so we don't have to wait ~60 seconds for the query
            # to finish
            try:
                member = await guild.fetch_member(user_id)
            except discord.HTTPException:
                return None
        else:
            members = await guild.query_members(limit=1, user_ids=[user_id],
                                                cache=True)
            if members is None or len(members) != 1:
                return None
            member = next(iter(members))

        guild._add_member(member, force=True)
        return member

    async def start(self, *args, **kwargs):
        await self.storage.init()
        await self.http_session.__aenter__()
        log.info('Starting bot...')
        await super().start(*args, **kwargs)

    async def close(self):
        await super().close()
        await self.http_session.__aexit__(None, None, None)

    async def on_guild_available(self, guild):
        log.info(f'Guild Available: {guild.id}')
        await guild.refresh_config()

    async def on_ready(self):
        log.info(f'Bot Ready: {self.user.name} ({self.user.id})')

    async def on_message(self, message):
        if message.author.bot:
            return
        await self.process_commands(message)

    async def on_guild_remove(self, guild):
        await guild.destroy()

    async def get_prefix(self, message):
        if isinstance(message, fake.FakeMessage):
            return ''
        return await super().get_prefix(message)

    def get_context(self, msg, *args, **kwargs):
        if isinstance(msg, fake.FakeMessage):
            msg._state = self._connection
        return super().get_context(msg, cls=HouraiContext, **kwargs)

    def get_automated_context(self, **kwargs):
        """
        Creates a fake context for automated uses. Mainly used to automatically
        run commands in response to configured triggers.
        """
        return self.get_context(fake.FakeMessage(**kwargs))

    async def process_commands(self, msg):
        if msg.author.bot:
            return

        ctx = await self.get_context(msg)

        if not ctx.valid or ctx.prefix is None:
            return

        async with ctx:
            await self.invoke(ctx)
        log.debug(f'Command successfully executed: {msg}')

    def add_cog(self, cog):
        super().add_cog(cog)
        log.info(f"Cog {cog.__class__.__name__} loaded.")

    async def on_error(self, event, *args, **kwargs):
        try:
            _, err, _ = sys.exc_info()
            err_msg = f'Error in {event} (args={args}, kwargs={kwargs}):'
            self.logger.exception(err_msg)
            self.dispatch('log_error', err_msg, err)
        except Exception:
            self.logger.exception('Waduhek')

    async def on_command_error(self, ctx, error):
        err_msg = None
        if isinstance(error, commands.CheckFailure):
            err_msg = str(error)
        elif isinstance(error, commands.UserInputError):
            err_msg = (str(error) + '\n') or ''
            err_msg += f"Try `~help {ctx.command} for a reference."
        elif isinstance(error, commands.CommandInvokeError):
            err_msg = ('An unexpected error has occured and has been reported.'
                       '\nIf this happens consistently, please consider filing'
                       ' a bug:\n<https://github.com/james7132/Hourai/issues>')
        if not err_msg:
            return

        prefix = (f"{ctx.author.mention} An error occured, and the bot does "
                  f"not have permissions to respond in this channel. Please "
                  f"double check the bot's permissions and try again. "
                  f"Original error message:\n\n")

        async def find_viable_channel(msg):
            if ctx.guild is None:
                return
            for channel in ctx.guild.text_channels:
                if channel.permissions_for(ctx.guild.me).send_messages and \
                   channel.permissions_for(ctx.author).read_messages:
                    await channel.send(prefix + msg)
                    return

        attempts = [
            lambda msg: ctx.send(msg),
            lambda msg: ctx.author.send(prefix + msg),
            find_viable_channel,
        ]

        for attempt in attempts:
            try:
                attempt(err_msg)
            except (discord.Forbidden, discord.NotFound):
                continue

    def load_extension(self, module):
        try:
            super().load_extension(module)
            self.logger.info(f'Loaded extension: {module}')
        except Exception:
            self.logger.exception(f'Failed to load extension: {module}')

    def load_all_extensions(self, base_module=extensions):
        disabled_extensions = self.get_config_value('disabled_extensions',
                                                    type=tuple, default=())
        modules = pkgutil.iter_modules(base_module.__path__,
                                       base_module.__name__ + '.')
        for module in modules:
            if module.name not in disabled_extensions:
                self.load_extension(module.name)

    def spin_wait_until_ready(self):
        while not self.is_ready():
            pass

    def get_config_value(self, *args, **kwargs):
        return config.get_config_value(self.config, *args, **kwargs)


class HouraiHelpCommand(commands.DefaultHelpCommand):

    async def send_bot_help(self, mapping):
        bot = self.context.bot
        command_name = self.clean_prefix + self.invoked_with

        response = (
            f"**{bot.user.name}**\n"
            f"{bot.description}\n"
            f"For a full list of available commands, please see "
            f"<https://docs.hourai.gg/Commands>.\n"
            f"For more detailed usage information on any command, use "
            f"`{command_name} <command>`.\n\n"
            f"{bot.user.name} is a bot focused on automating security and "
            f"moderation with extensive configuration options. Most of the "
            f"advanced features are not directly accessible via commands. "
            f"Please see the full documentation at <https://docs.hourai.gg/>."
            f"\n\n If you find this bot useful, please vote for the bot: "
            f"<https://top.gg/bot/{bot.user.id}>")

        await self.context.send(response)
