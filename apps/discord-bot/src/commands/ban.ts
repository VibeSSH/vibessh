import { SlashCommandBuilder, PermissionFlagsBits, type ChatInputCommandInteraction } from "discord.js";
import { panel } from "../lib/components.js";
import { replyPanel } from "../lib/reply.js";
import { COLOR } from "../lib/theme.js";
import { strings } from "../i18n.js";
import { log } from "../lib/logger.js";
import type { BotCommand } from "./types.js";

/**
 * `/ban` - remove someone and keep them out, with a reason.
 *
 * Team-only through `BanMembers` as the default member permission, so it never
 * shows up for ordinary members. The target is DMed the reason first (best
 * effort - a closed DM must not stop the ban), then banned.
 */
export const banCommand: BotCommand = {
  data: new SlashCommandBuilder()
    .setName("ban")
    .setDescription("Zbanuj użytkownika / Ban a member")
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .setDMPermission(false)
    .addUserOption((option) => option.setName("user").setDescription("Kogo / Who").setRequired(true))
    .addStringOption((option) => option.setName("reason").setDescription("Powód / Reason").setRequired(false)) as SlashCommandBuilder,

  async execute(interaction: ChatInputCommandInteraction) {
    if (!interaction.inCachedGuild()) return;
    const user = interaction.options.getUser("user", true);
    const reason = interaction.options.getString("reason") ?? "-";

    // Tell them why before they lose access to the channel where they'd read it.
    await user.send(`${strings.pl.moderation.banned(reason)}\n${strings.en.moderation.banned(reason)}`).catch(() => undefined);

    try {
      await interaction.guild.members.ban(user.id, { reason: `${interaction.user.tag}: ${reason}` });
      await replyPanel(
        interaction,
        panel({
          accent: COLOR.danger,
          title: "🔨 Ban",
          body: `**${user.tag}** został zbanowany · has been banned.`,
          fields: [{ label: "Powód · Reason", value: reason }],
        }),
      );
    } catch (error) {
      log.error(`ban failed for ${user.id}`, error);
      await replyPanel(
        interaction,
        panel({ accent: COLOR.danger, body: "Nie udało się zbanować - sprawdź rolę/uprawnienia bota. / Couldn't ban - check the bot's role and permissions." }),
        { ephemeral: true },
      );
    }
  },
};

/** `/unban` - lift a ban by user id (they aren't in the server to pick from a list). */
export const unbanCommand: BotCommand = {
  data: new SlashCommandBuilder()
    .setName("unban")
    .setDescription("Odbanuj użytkownika po ID / Unban a member by id")
    .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers)
    .setDMPermission(false)
    .addStringOption((option) => option.setName("user_id").setDescription("ID użytkownika / User id").setRequired(true)) as SlashCommandBuilder,

  async execute(interaction: ChatInputCommandInteraction) {
    if (!interaction.inCachedGuild()) return;
    const userId = interaction.options.getString("user_id", true).trim();
    if (!/^\d{16,20}$/.test(userId)) {
      await replyPanel(interaction, panel({ accent: COLOR.danger, body: "To nie wygląda na ID użytkownika. / That doesn't look like a user id." }), { ephemeral: true });
      return;
    }
    try {
      await interaction.guild.members.unban(userId, `${interaction.user.tag}: unban`);
      await replyPanel(interaction, panel({ accent: COLOR.success, body: `Odbanowano \`${userId}\` · unbanned.` }));
    } catch (error) {
      log.error(`unban failed for ${userId}`, error);
      await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Nie udało się odbanować (może nie był zbanowany). / Couldn't unban (maybe not banned)." }), { ephemeral: true });
    }
  },
};
