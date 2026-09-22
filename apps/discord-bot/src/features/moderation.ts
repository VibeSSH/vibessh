import { PermissionFlagsBits, type Message, type GuildMember, type GuildTextBasedChannel } from "discord.js";
import { ids } from "../store.js";
import { strings } from "../i18n.js";
import { TEAM_ROLE_KEYS } from "../setup/blueprint.js";
import { log } from "../lib/logger.js";

// A short burst that trips the spam guard, and how long the offender is muted
// when it does. Generous enough that a normal fast chat never trips it.
const SPAM_WINDOW_MS = 6000;
const SPAM_LIMIT = 6;
const SPAM_TIMEOUT_MS = 60_000;

// Links (any scheme, bare www., or a Discord invite) - the anti-link rule keeps
// them to the team. Gif hosts are matched separately so they can be allowed in
// the chat channel.
const LINK_RE = /(https?:\/\/|www\.)\S+|discord\.gg\/\S+|discord\.com\/invite\/\S+/i;
const GIF_LINK_RE = /(tenor\.com|giphy\.com|\.gif(\?|$))/i;

/** How many messages each author has sent recently, for the spam window. */
const recent = new Map<string, number[]>();

/** A message is exempt from every filter when its author is the bot, a team
 *  member, or anyone who can already manage messages here. */
function isExempt(member: GuildMember | null): boolean {
  if (!member || member.user.bot) return true;
  if (member.permissions.has(PermissionFlagsBits.ManageMessages)) return true;
  const teamRoleIds = new Set(TEAM_ROLE_KEYS.map((key) => ids.role(key)).filter(Boolean));
  return member.roles.cache.some((role) => teamRoleIds.has(role.id));
}

function isChatChannel(channelId: string): boolean {
  return channelId === ids.channel("pl-general") || channelId === ids.channel("en-general");
}

function isGifOrSticker(message: Message): boolean {
  if (message.stickers.size > 0) return true;
  if (message.attachments.some((a) => a.contentType?.includes("gif") || /\.gif$/i.test(a.name ?? ""))) return true;
  if (message.embeds.some((e) => e.data.type === "gifv" || (e.url ? GIF_LINK_RE.test(e.url) : false))) return true;
  return GIF_LINK_RE.test(message.content);
}

/** Posts a short bilingual notice that removes itself, so the channel is not
 *  left with a moderation message sitting under a deleted one. */
async function warn(channel: GuildTextBasedChannel, pl: string, en: string): Promise<void> {
  if (!channel.isSendable()) return;
  const notice = await channel.send({ content: `🛡️ ${pl}\n${en}` }).catch(() => null);
  if (notice) setTimeout(() => void notice.delete().catch(() => undefined), 6000);
}

function tripsSpam(userId: string): boolean {
  const now = Date.now();
  const times = (recent.get(userId) ?? []).filter((t) => now - t < SPAM_WINDOW_MS);
  times.push(now);
  recent.set(userId, times);
  return times.length > SPAM_LIMIT;
}

/**
 * The message filter: anti-link, gif/sticker containment, and anti-spam.
 *
 * Runs on every guild message. Exempt authors (the team, anyone who can manage
 * messages) pass untouched. Links are kept to the team; gifs and stickers are
 * allowed only in the chat channel; a burst of messages is deleted and the
 * author muted briefly. Every deletion leaves a short self-erasing note so the
 * person knows why, without cluttering the channel.
 */
export async function moderateMessage(message: Message): Promise<void> {
  if (!message.inGuild() || isExempt(message.member)) return;
  const channel = message.channel as GuildTextBasedChannel;
  const m = strings; // both languages, for the bilingual notices

  try {
    const gif = isGifOrSticker(message);
    if (gif && !isChatChannel(message.channelId)) {
      await message.delete();
      await warn(channel, m.pl.moderation.gifOnlyInChat, m.en.moderation.gifOnlyInChat);
      return;
    }
    if (!gif && LINK_RE.test(message.content)) {
      await message.delete();
      await warn(channel, m.pl.moderation.linkBlocked, m.en.moderation.linkBlocked);
      return;
    }
    if (tripsSpam(message.author.id)) {
      await message.delete().catch(() => undefined);
      await message.member?.timeout(SPAM_TIMEOUT_MS, "Spam").catch(() => undefined);
      await warn(channel, m.pl.moderation.spamBlocked, m.en.moderation.spamBlocked);
    }
  } catch (error) {
    // A message deleted by someone else first, or a missing-permission delete,
    // is not worth a crash - the filter is best-effort.
    log.error("moderation action failed", error);
  }
}
