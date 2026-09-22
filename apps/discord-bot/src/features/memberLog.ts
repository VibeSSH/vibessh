import { type Guild, type GuildMember, type PartialGuildMember } from "discord.js";
import { panel } from "../lib/components.js";
import { sendPanel } from "../lib/reply.js";
import { COLOR } from "../lib/theme.js";
import { ids } from "../store.js";
import { log } from "../lib/logger.js";

// The logs channel the join/leave notices go to. Prefers the one the setup
// recorded; falls back to the id given for this server so it works even before
// a fresh `/setup`.
const FALLBACK_LOG_CHANNEL = "1551582871884406896";

function logChannel(guild: Guild) {
  const id = ids.channel("logs") ?? FALLBACK_LOG_CHANNEL;
  const channel = guild.channels.cache.get(id);
  return channel?.isTextBased() ? channel : null;
}

/** Greets a new member in the logs channel, with when their account was made
 *  and the fresh member count. */
export async function handleMemberJoin(member: GuildMember): Promise<void> {
  const channel = logChannel(member.guild);
  if (!channel) return;
  const created = Math.floor(member.user.createdTimestamp / 1000);
  try {
    await sendPanel(
      channel,
      panel({
        accent: COLOR.success,
        title: "🎉 Nowy członek · New member",
        body: `${member} · **${member.user.tag}**\n\n🇵🇱 Witamy na serwerze VibeSSH!\n🇬🇧 Welcome to the VibeSSH server!`,
        thumbnail: member.user.displayAvatarURL({ size: 128 }),
        fields: [
          { label: "Konto utworzone · Account created", value: `<t:${created}:R>` },
          { label: "Członków · Members", value: `${member.guild.memberCount}` },
        ],
      }),
    );
  } catch (error) {
    log.error("member join log failed", error);
  }
}

/** Notes a departure in the same channel. */
export async function handleMemberLeave(member: GuildMember | PartialGuildMember): Promise<void> {
  const channel = logChannel(member.guild);
  if (!channel) return;
  try {
    await sendPanel(
      channel,
      panel({
        accent: COLOR.neutral,
        title: "👋 Odszedł · Left",
        body: `**${member.user?.tag ?? member.id}**\n\n🇵🇱 Opuścił serwer.\n🇬🇧 Left the server.`,
        thumbnail: member.user?.displayAvatarURL({ size: 128 }),
      }),
    );
  } catch (error) {
    log.error("member leave log failed", error);
  }
}
