import {
  ButtonBuilder,
  ButtonStyle,
  ChannelType,
  PermissionFlagsBits,
  type ButtonInteraction,
  type CategoryChannel,
  type OverwriteResolvable,
} from "discord.js";
import { panel } from "../lib/components.js";
import { replyPanel, sendPanel } from "../lib/reply.js";
import { COLOR } from "../lib/theme.js";
import { ids } from "../store.js";
import { strings, type Lang } from "../i18n.js";
import { TEAM_ROLE_KEYS } from "../setup/blueprint.js";
import { log } from "../lib/logger.js";

const OPEN_PREFIX = "ticket:open:";
const CLOSE_ID = "ticket:close";
/** The banner shown at the top of each language's ticket panel. */
const TICKET_BANNER: Record<Lang, string> = {
  pl: "https://i.imgur.com/j5qYa4J.png",
  en: "https://i.imgur.com/ZwBgKHn.png",
};
// A ticket channel carries the opener's id in its topic, so "do you already
// have one open" is a lookup rather than a name guess, and the closer knows
// whose ticket it is.
const TOPIC_PREFIX = "vibessh-ticket:";

/** The standing panel a support channel shows: one button that opens a ticket
 *  in that channel's language. */
export function buildTicketPanel(lang: Lang) {
  const s = strings[lang].ticketPanel;
  return panel({
    accent: COLOR.accent,
    headerImage: TICKET_BANNER[lang],
    title: `🎫 ${s.title}`,
    body: s.body,
    buttons: [new ButtonBuilder().setCustomId(`${OPEN_PREFIX}${lang}`).setLabel(s.button).setStyle(ButtonStyle.Primary).setEmoji("🎫")],
  });
}

export function isTicketButton(customId: string): boolean {
  return customId.startsWith(OPEN_PREFIX) || customId === CLOSE_ID;
}

export async function handleTicketButton(interaction: ButtonInteraction): Promise<void> {
  if (interaction.customId === CLOSE_ID) return closeTicket(interaction);
  return openTicket(interaction);
}

async function openTicket(interaction: ButtonInteraction): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const lang = interaction.customId.slice(OPEN_PREFIX.length) as Lang;
  const s = strings[lang].ticketPanel;
  const guild = interaction.guild;
  const opener = interaction.user;

  // One open ticket per person: a second click points at the first rather
  // than spawning a pile of channels.
  const existing = guild.channels.cache.find(
    (channel) => channel.type === ChannelType.GuildText && channel.topic === `${TOPIC_PREFIX}${opener.id}`,
  );
  if (existing) {
    await replyPanel(interaction, panel({ accent: COLOR.warning, body: s.alreadyOpen.replace("{channel}", `<#${existing.id}>`) }), { ephemeral: true });
    return;
  }

  const parent = supportCategory(guild, lang);
  const teamRoleIds = TEAM_ROLE_KEYS.map((key) => ids.role(key)).filter((id): id is string => Boolean(id));
  const overwrites: OverwriteResolvable[] = [
    { id: guild.id, deny: [PermissionFlagsBits.ViewChannel] },
    { id: opener.id, allow: [PermissionFlagsBits.ViewChannel, PermissionFlagsBits.SendMessages, PermissionFlagsBits.AttachFiles] },
    ...teamRoleIds.map((id) => ({ id, allow: [PermissionFlagsBits.ViewChannel, PermissionFlagsBits.SendMessages] })),
  ];

  try {
    const channel = await guild.channels.create({
      name: `ticket-${opener.username}`.slice(0, 90),
      type: ChannelType.GuildText,
      parent: parent?.id,
      topic: `${TOPIC_PREFIX}${opener.id}`,
      permissionOverwrites: overwrites,
      reason: `Ticket opened by ${opener.tag}`,
    });

    const intro = panel({
      accent: COLOR.accent,
      title: s.createdTitle,
      body: `${opener}, ${s.createdBody}`,
      buttons: [new ButtonBuilder().setCustomId(CLOSE_ID).setLabel(s.close).setStyle(ButtonStyle.Danger).setEmoji("🔒")],
    });
    await sendPanel(channel, intro);

    await replyPanel(interaction, panel({ accent: COLOR.success, body: `${s.createdTitle}: <#${channel.id}>` }), { ephemeral: true });
  } catch (error) {
    log.error(`failed to open ticket for ${opener.id}`, error);
    await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Couldn't open a ticket - the bot may lack Manage Channels here. Ping an admin." }), { ephemeral: true });
  }
}

async function closeTicket(interaction: ButtonInteraction): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const channel = interaction.channel;
  if (!channel || channel.type !== ChannelType.GuildText || !channel.topic?.startsWith(TOPIC_PREFIX)) {
    await replyPanel(interaction, panel({ accent: COLOR.danger, body: "This isn't a ticket channel." }), { ephemeral: true });
    return;
  }
  // The panel's language is not carried on the button, so answer in both.
  await replyPanel(interaction, panel({ accent: COLOR.neutral, body: `${strings.pl.ticketPanel.closed}\n${strings.en.ticketPanel.closed}` }));
  // A short beat so whoever pressed it sees the confirmation before the
  // channel disappears under them.
  setTimeout(() => {
    void channel.delete(`Ticket closed by ${interaction.user.tag}`).catch((error) => log.error("failed to delete ticket channel", error));
  }, 4000);
}

function supportCategory(guild: ButtonInteraction["guild"], lang: Lang): CategoryChannel | undefined {
  if (!guild) return undefined;
  const supportId = ids.channel(lang === "pl" ? "pl-support" : "en-support");
  const support = supportId ? guild.channels.cache.get(supportId) : undefined;
  if (support && "parent" in support && support.parent && support.parent.type === ChannelType.GuildCategory) {
    return support.parent;
  }
  return undefined;
}
