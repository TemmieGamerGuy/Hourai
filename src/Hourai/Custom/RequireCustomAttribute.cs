using Discord;
using Discord.Commands;
using System.Threading.Tasks;

namespace Hourai.Custom {

  public class RequireCustomAttribute : PreconditionAttribute {

    public override Task<PreconditionResult> CheckPermissions(
        ICommandContext context,
        CommandInfo commandInfo,
        IDependencyMap dependencies) {
      var houraiContext = context as HouraiContext;
      if (houraiContext == null || !houraiContext.IsAutoCommand)
        return Task.FromResult(
          PreconditionResult.FromError("Command executable only via custom config."));
      return Task.FromResult(PreconditionResult.FromSuccess());
    }

  }

}