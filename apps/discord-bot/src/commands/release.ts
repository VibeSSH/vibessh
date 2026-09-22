import { SlashCommandBuilder, PermissionFlagsBits, MessageFlags, type ChatInputCommandInteraction, type GuildTextBasedChannel } from "discord.js";
import { buildReleasePanel } from "../features/release.js";
import { panel } from "../lib/components.js";
import { replyPanel } from "../lib/reply.js";
import { COLOR } from "../lib/theme.js";
import { ids } from "../store.js";
import { log } from "../lib/logger.js";
import type { BotCommand } from "./types.js";

/**
 * `/release <version>` - announce a new VibeSSH version in #releases.
 *
 * Posts the bilingual release card and pings the Release Notifications role
 * (whoever opted in). Team-only via `ManageGuild`. Manual for now; once a
 * version feed exists (the status endpoint) this can also fire automatically on
 * a version change.
 */
export const releaseCommand: BotCommand = {
  data: new SlashCommandBuilder()
    .setName("release")
    .setDescription("Ogłoś nową wersję w #releases / Announce a new release")
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
    .setDMPermission(false)
    .addStringOption((option) => option.setName("version").setDescription("np. 0.1.0-beta.19").setRequired(true)) as SlashCommandBuilder,

  async execute(interaction: ChatInputCommandInteraction) {
    if (!interaction.inCachedGuild()) return;
    const version = interaction.options.getString("version", true).trim();

    const channelId = ids.channel("releases");
    const channel = channelId ? await interaction.guild.channels.fetch(channelId).catch(() => null) : null;
    if (!channel?.isTextBased()) {
      await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Brak kanału #releases - odpal najpierw `/setup`. / No #releases channel - run `/setup` first." }), { ephemeral: true });
      return;
    }

    const roleId = ids.role("notify-release");
    try {
      await (channel as GuildTextBasedChannel).send({
        components: [buildReleasePanel(version, roleId)],
        flags: MessageFlags.IsComponentsV2,
        allowedMentions: { roles: roleId ? [roleId] : [] },
      });
      await replyPanel(interaction, panel({ accent: COLOR.success, body: `Ogłoszono \`${version}\` w <#${channel.id}>.` }), { ephemeral: true });
    } catch (error) {
      log.error("release announcement failed", error);
      await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Nie udało się ogłosić - sprawdź uprawnienia bota na #releases. / Couldn't announce - check the bot's perms in #releases." }), { ephemeral: true });
    }
  },
};
