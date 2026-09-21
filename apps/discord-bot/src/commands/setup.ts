import { SlashCommandBuilder, PermissionFlagsBits, MessageFlags, type Guild, type ContainerBuilder, type ChatInputCommandInteraction, type GuildTextBasedChannel } from "discord.js";
import { reconcile, summarize } from "../setup/reconcile.js";
import { buildLanguagePanel } from "../features/language.js";
import { buildTicketPanel } from "../features/tickets.js";
import { buildTermsPanel, buildAboutPanel } from "../features/infoPanels.js";
import { buildNotificationsPanel } from "../features/notifications.js";
import { upsertPanel } from "../lib/panels.js";
import { panel } from "../lib/components.js";
import { replyPanel } from "../lib/reply.js";
import { COLOR } from "../lib/theme.js";
import { ids } from "../store.js";
import { LANGS } from "../i18n.js";
import { log } from "../lib/logger.js";
import type { BotCommand } from "./types.js";

/** Posts (or refreshes) one panel into the channel behind a blueprint key.
 *  A missing channel is skipped quietly - setup should still finish the rest. */
async function postPanel(guild: Guild, channelKey: string, panelKey: string, container: ContainerBuilder): Promise<void> {
  const channelId = ids.channel(channelKey);
  if (!channelId) return;
  const channel = await guild.channels.fetch(channelId).catch(() => null);
  if (channel?.isTextBased()) {
    await upsertPanel(channel as GuildTextBasedChannel, panelKey, container);
  }
}

/** Panels an earlier version posted per-language and this one merged into one
 *  bilingual card - their old messages are deleted on setup so they don't
 *  linger beside the new combined ones. */
const RETIRED_PANELS: { channelKey: string; panelKey: string }[] = [
  { channelKey: "rules", panelKey: "panel:terms:pl" },
  { channelKey: "rules", panelKey: "panel:terms:en" },
  { channelKey: "welcome", panelKey: "panel:about:pl" },
  { channelKey: "welcome", panelKey: "panel:about:en" },
];

async function retireOldPanels(guild: Guild): Promise<void> {
  for (const { channelKey, panelKey } of RETIRED_PANELS) {
    const messageId = ids.message(panelKey);
    const channelId = ids.channel(channelKey);
    if (messageId && channelId) {
      const channel = await guild.channels.fetch(channelId).catch(() => null);
      if (channel?.isTextBased()) {
        const message = await channel.messages.fetch(messageId).catch(() => null);
        await message?.delete().catch(() => undefined);
      }
    }
    ids.clearMessage(panelKey);
  }
}

/** Every standing panel the server shows, posted to its channel. Separate from
 *  role/channel reconciliation so one failing panel never rolls back the setup.
 *  The info panels (terms, about) are one bilingual card each in the public
 *  START HERE channels; the ticket panels stay per-language, since their
 *  channels are. */
async function postAllPanels(guild: Guild): Promise<void> {
  await retireOldPanels(guild);
  await postPanel(guild, "choose-language", "panel:language", buildLanguagePanel());
  await postPanel(guild, "rules", "panel:terms", buildTermsPanel());
  await postPanel(guild, "welcome", "panel:about", await buildAboutPanel());
  await postPanel(guild, "releases", "panel:notifications", buildNotificationsPanel());
  for (const lang of LANGS) {
    await postPanel(guild, lang === "pl" ? "pl-support" : "en-support", `panel:ticket:${lang}`, buildTicketPanel(lang));
  }
}

/**
 * `/setup` - the whole self-configuration in one command.
 *
 * Reconciles roles, channels and permissions to the blueprint, then (re)posts
 * the standing panels the server needs. Admin-only, and deferred because a
 * first run on a fresh server makes a lot of API calls.
 */
export const setupCommand: BotCommand = {
  data: new SlashCommandBuilder()
    .setName("setup")
    .setDescription("Skonfiguruj role, kanały i uprawnienia wg blueprintu VibeSSH / Configure the server.")
    .setDefaultMemberPermissions(PermissionFlagsBits.Administrator)
    .setDMPermission(false),

  async execute(interaction: ChatInputCommandInteraction) {
    if (!interaction.inCachedGuild()) return;
    await interaction.deferReply({ flags: MessageFlags.Ephemeral });

    try {
      const summary = await reconcile(interaction.guild);
      await postAllPanels(interaction.guild);

      await replyPanel(interaction, panel({ accent: COLOR.success, title: "✅ Konfiguracja gotowa", body: summarize(summary) }), { ephemeral: true });
    } catch (error) {
      log.error("setup failed", error);
      await replyPanel(
        interaction,
        panel({
          accent: COLOR.danger,
          title: "Setup nie przeszedł do końca",
          body: "Sprawdź, czy rola bota jest **nad** rolami, które nadaje, i ma uprawnienia **Zarządzaj rolami/kanałami**. Szczegóły w logach bota.",
        }),
        { ephemeral: true },
      );
    }
  },
};
