import {
  ButtonBuilder,
  ButtonStyle,
  ContainerBuilder,
  TextDisplayBuilder,
  MediaGalleryBuilder,
  MediaGalleryItemBuilder,
  SeparatorBuilder,
  SeparatorSpacingSize,
  ActionRowBuilder,
  type MessageActionRowComponentBuilder,
  type ButtonInteraction,
  type GuildMember,
} from "discord.js";
import { panel, normalizeImageUrl } from "../lib/components.js";
import { replyPanel } from "../lib/reply.js";
import { COLOR } from "../lib/theme.js";
import { ids } from "../store.js";
import { log } from "../lib/logger.js";

const PREFIX = "notify:toggle:";
const ROLE_KEY = { release: "notify-release", testing: "notify-testing" } as const;
type Which = keyof typeof ROLE_KEY;

const NOTIF_BANNER = {
  pl: "https://i.imgur.com/ERs1cEJ.png",
  en: "https://i.imgur.com/QJ1ESfv.png",
} as const;

/** The self-service panel: two buttons that add or remove the notification
 *  roles, so the release ping reaches people who actually asked for it. One
 *  bilingual card with a banner and copy per language. */
export function buildNotificationsPanel() {
  const small = () => new SeparatorBuilder().setSpacing(SeparatorSpacingSize.Small);
  const banner = (url: string) => new MediaGalleryBuilder().addItems(new MediaGalleryItemBuilder().setURL(normalizeImageUrl(url)));

  return new ContainerBuilder()
    .setAccentColor(COLOR.accent)
    .addTextDisplayComponents(new TextDisplayBuilder().setContent("## 🔔 Powiadomienia · Notifications"))
    .addMediaGalleryComponents(banner(NOTIF_BANNER.pl))
    .addTextDisplayComponents(
      new TextDisplayBuilder().setContent("🇵🇱 Kliknij, żeby włączyć lub wyłączyć pingi. **Release** - nowe wydania, **Testing** - wersje testowe."),
    )
    .addSeparatorComponents(small())
    .addMediaGalleryComponents(banner(NOTIF_BANNER.en))
    .addTextDisplayComponents(
      new TextDisplayBuilder().setContent("🇬🇧 Click to turn pings on or off. **Release** - new releases, **Testing** - testing builds."),
    )
    .addSeparatorComponents(small())
    .addActionRowComponents(
      new ActionRowBuilder<MessageActionRowComponentBuilder>().addComponents(
        new ButtonBuilder().setCustomId(`${PREFIX}release`).setLabel("Release Notifications").setStyle(ButtonStyle.Secondary).setEmoji("🔔"),
        new ButtonBuilder().setCustomId(`${PREFIX}testing`).setLabel("Testing Notifications").setStyle(ButtonStyle.Secondary).setEmoji("🧪"),
      ),
    );
}

export function isNotifyButton(customId: string): boolean {
  return customId.startsWith(PREFIX);
}

/** Toggles the chosen notification role on the presser, and says which way it
 *  went - so one button both opts in and out. */
export async function handleNotifyButton(interaction: ButtonInteraction): Promise<void> {
  if (!interaction.inCachedGuild()) return;
  const which = interaction.customId.slice(PREFIX.length) as Which;
  const roleId = ids.role(ROLE_KEY[which]);
  if (!roleId) {
    await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Role powiadomień nie są jeszcze gotowe - `/setup`. / Notification roles aren't set up yet - `/setup`." }), { ephemeral: true });
    return;
  }

  const member = interaction.member as GuildMember;
  try {
    if (member.roles.cache.has(roleId)) {
      await member.roles.remove(roleId, "Notifications opt-out");
      await replyPanel(interaction, panel({ accent: COLOR.neutral, body: "🔕 Wyłączono powiadomienia. · Notifications off." }), { ephemeral: true });
    } else {
      await member.roles.add(roleId, "Notifications opt-in");
      await replyPanel(interaction, panel({ accent: COLOR.success, body: "🔔 Włączono powiadomienia. · Notifications on." }), { ephemeral: true });
    }
  } catch (error) {
    log.error(`notify toggle failed (${which}) for ${member.id}`, error);
    await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Nie udało się zmienić - rola bota musi być nad rolami powiadomień. / Couldn't change it - the bot's role must sit above the notification roles." }), { ephemeral: true });
  }
}
