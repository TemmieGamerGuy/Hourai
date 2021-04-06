import logging
from . import proto, models


log = logging.getLogger(__name__)


class BanStorage:
    """An interface for access store all of the bans seen by the bot."""

    def __init__(self, storage):
        self.storage = storage

    def get_guild_bans(self, guild_id):
        session = self.storage.create_session()
        with session:
            bans = session.query(models.Ban) \
                          .filter_by(guild_id=guild_id) \
                          .all()
            return list(self._make_ban_protos(bans, session))

    def get_user_bans(self, user_id):
        session = self.storage.create_session()
        with session:
            bans = session.query(models.Ban) \
                          .filter_by(user_id=user_id) \
                          .all()
            return list(self._make_ban_protos(bans, session))

    def _make_ban_protos(self, bans, session):
        guild_ids = set(b.guild_id for b in bans)
        configs = session.query(models.AdminConfig) \
                         .filter(models.AdminConfig.id.in_(guild_ids)) \
                         .all()
        configs = {cfg.id: cfg for cfg in configs}
        for ban in bans:
            config = configs.get(ban.guild_id)
            ban_proto = proto.BanInfo()
            ban_proto.guild_id = ban.guild_id
            ban_proto.user_id = ban.user_id
            ban_proto.guild_size = \
                session.query(models.Member) \
                       .filter_by(guild_id=ban.guild_id, bot=False) \
                       .count()
            if ban.reason is not None:
                ban_proto.reason = ban.reason
            if config is not None:
                ban_proto.guild_blocked = config.is_blocked
            yield ban_proto
