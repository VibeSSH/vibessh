import {
  SlashCommandBuilder,
  PermissionFlagsBits,
  MessageFlags,
  ChannelType,
  type ChatInputCommandInteraction,
  type TextChannel,
} from "discord.js";
import { panel } from "../lib/components.js";
import { replyPanel } from "../lib/reply.js";
import { COLOR } from "../lib/theme.js";
import { ids } from "../store.js";
import { log } from "../lib/logger.js";
import type { BotCommand } from "./types.js";

const WEBHOOK_NAME = "GitHub";
// The dev-logs channel to hook up when neither an option nor the setup store
// names one - the id given for this server.
const FALLBACK_CHANNEL = "1551670775336140960";

/**
 * `/devlogs` - wire GitHub commits into #dev-logs.
 *
 * Creates (or reuses) a webhook on the channel and hands back its URL with
 * `/github` appended - the endpoint Discord formats GitHub's push and PR
 * payloads through. The reply is ephemeral because that URL is write access to
 * the channel. The GitHub side (repo -> Settings -> Webhooks) is the one manual
 * step left, and the reply says exactly what to paste.
 */
export const devlogsCommand: BotCommand = {
  data: new SlashCommandBuilder()
    .setName("devlogs")
    .setDescription("Podłącz commity GitHub do #dev-logs / Wire GitHub commits into #dev-logs")
    .setDefaultMemberPermissions(PermissionFlagsBits.ManageWebhooks)
    .setDMPermission(false)
    .addChannelOption((option) =>
      option.setName("channel").setDescription("Kanał docelowy / Target channel").addChannelTypes(ChannelType.GuildText).setRequired(false),
    ) as SlashCommandBuilder,

  async execute(interaction: ChatInputCommandInteraction) {
    if (!interaction.inCachedGuild()) return;
    await interaction.deferReply({ flags: MessageFlags.Ephemeral });

    const channelId = interaction.options.getChannel("channel")?.id ?? ids.channel("dev-logs") ?? FALLBACK_CHANNEL;
    const channel = await interaction.guild.channels.fetch(channelId).catch(() => null);
    if (!channel || channel.type !== ChannelType.GuildText) {
      await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Nie znalazłem kanału tekstowego - odpal `/setup` albo podaj kanał. / No text channel found - run `/setup` or pass one." }), { ephemeral: true });
      return;
    }

    try {
      const existing = await (channel as TextChannel).fetchWebhooks();
      const hook =
        existing.find((webhook) => webhook.name === WEBHOOK_NAME && webhook.owner?.id === interaction.client.user.id) ??
        (await (channel as TextChannel).createWebhook({ name: WEBHOOK_NAME, reason: "GitHub dev-logs" }));

      await replyPanel(
        interaction,
        panel({
          accent: COLOR.accent,
          title: "🛠️ Webhook GitHub gotowy · ready",
          body: [
            `Kanał · Channel: <#${channel.id}>`,
            "",
            "Wklej ten URL w **GitHub → repozytorium → Settings → Webhooks → Add webhook** · paste it there:",
            `\`\`\`${hook.url}/github\`\`\``,
            "Content type: `application/json`, wybierz eventy (Pushes, Pull requests) · pick events, then save.",
            "-# Traktuj URL jak sekret - kto go ma, może pisać na tym kanale. · Treat the URL like a secret.",
          ].join("\n"),
        }),
        { ephemeral: true },
      );
    } catch (error) {
      log.error("devlogs webhook failed", error);
      await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Nie udało się - bot potrzebuje uprawnienia **Zarządzaj webhookami** na tym kanale. / Failed - the bot needs **Manage Webhooks** here." }), { ephemeral: true });
    }
  },
};
