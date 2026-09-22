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
} from "discord.js";
import { normalizeImageUrl } from "../lib/components.js";
import { COLOR } from "../lib/theme.js";
import { strings } from "../i18n.js";

const RELEASE_BANNER = {
  pl: "https://i.imgur.com/OjBpBNy.png",
  en: "https://i.imgur.com/WJnx66S.png",
} as const;
const DOWNLOAD_URL = "https://vibessh.dev/download";

/**
 * The release announcement posted in #releases.
 *
 * One bilingual card - a banner and copy per language - headed by the new
 * version and led (when the role exists) by a mention of the Release
 * Notifications role, so opted-in members are pinged. The mention lives in a
 * text component because a Components V2 message carries no `content`; the
 * caller pairs it with `allowedMentions` so it actually notifies.
 */
export function buildReleasePanel(version: string, roleId?: string): ContainerBuilder {
  const small = () => new SeparatorBuilder().setSpacing(SeparatorSpacingSize.Small);
  const banner = (url: string) => new MediaGalleryBuilder().addItems(new MediaGalleryItemBuilder().setURL(normalizeImageUrl(url)));
  const container = new ContainerBuilder().setAccentColor(COLOR.accent);

  if (roleId) container.addTextDisplayComponents(new TextDisplayBuilder().setContent(`<@&${roleId}>`));

  container
    .addTextDisplayComponents(
      new TextDisplayBuilder().setContent(`## 🚀 ${strings.pl.release.heading} · ${strings.en.release.heading}\n\`${version}\``),
    )
    .addMediaGalleryComponents(banner(RELEASE_BANNER.pl))
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`🇵🇱 ${strings.pl.release.body}`))
    .addSeparatorComponents(small())
    .addMediaGalleryComponents(banner(RELEASE_BANNER.en))
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`🇬🇧 ${strings.en.release.body}`))
    .addSeparatorComponents(small())
    .addActionRowComponents(
      new ActionRowBuilder<MessageActionRowComponentBuilder>().addComponents(
        new ButtonBuilder()
          .setStyle(ButtonStyle.Link)
          .setURL(DOWNLOAD_URL)
          .setLabel(`${strings.pl.release.download} · ${strings.en.release.download}`)
          .setEmoji("📥"),
      ),
    )
    .addSeparatorComponents(small())
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`-# ${new Date().toISOString().slice(0, 10)}`));

  return container;
}
