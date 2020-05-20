This is a public changelog of the alterations made to the bot, including both
code and operational changes.

Questions regarding the bot's use? Join the public development server for Hourai
here: https://discord.gg/UydKWHX.

### v1.1.0 (05/20/2020)

 * **Hourai is now a [Verified Discord Bot](https://support.discord.com/hc/en-us/articles/360040720412-Bot-Verification-and-Data-Whitelisting)**
 * [General] Added utilities for making public announcements via modlog channels.
   Will be used to communicate important information regarding the bot to server
   owners and moderators in the future.
 * [General] Improved `~help` documentation on multiple commands.
 * [Validation] Added `~validation verify` for running out-of-band validation.
 * [Validation] Added an Override level approver for approving owners of
   Partnered or Verified servers.
 * [Music] Fixed bug where non-DJ users could use `~stop`.
 * [Music] Improved stability of music bot features. Music bot should be able to
   sustain long queues lasting well over 7 hours.