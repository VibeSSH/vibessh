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
import { getInstallations, getStatus } from "../lib/status.js";
import { strings } from "../i18n.js";

/** When these documents were last published - shown in each panel's footer, as
 *  the spec asked. Bump it when the terms or the app description change. */
export const PUBLISHED_ISO = "2026-09-21";

const SITE = "https://vibessh.dev";

/** The localized banners shown above each language's copy. */
const ABOUT_BANNER = {
  pl: "https://i.imgur.com/wvHmplz.png",
  en: "https://i.imgur.com/r3JY09F.png",
} as const;
const TERMS_BANNER = {
  pl: "https://i.imgur.com/w0inISr.png",
  en: "https://i.imgur.com/QOM0qtu.png",
} as const;

function link(url: string, label: string, emoji?: string): ButtonBuilder {
  const button = new ButtonBuilder().setStyle(ButtonStyle.Link).setURL(url).setLabel(label);
  return emoji ? button.setEmoji(emoji) : button;
}

/**
 * The terms panel for the rules channel.
 *
 * One bilingual panel rather than a Polish one stacked on an English one: the
 * START HERE channels are public, seen before anyone has picked a language, so
 * both languages share a single card (a flag ahead of each). The
 * language-specific panels (the ticket ones) stay per-language, because their
 * channels already are.
 */
export function buildTermsPanel() {
  const small = () => new SeparatorBuilder().setSpacing(SeparatorSpacingSize.Small);
  const banner = (url: string) => new MediaGalleryBuilder().addItems(new MediaGalleryItemBuilder().setURL(normalizeImageUrl(url)));

  return new ContainerBuilder()
    .setAccentColor(COLOR.accent)
    .addTextDisplayComponents(new TextDisplayBuilder().setContent("## 📜 Regulamin · Terms of Use"))
    .addMediaGalleryComponents(banner(TERMS_BANNER.pl))
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`🇵🇱 ${strings.pl.terms.body}`))
    .addSeparatorComponents(small())
    .addMediaGalleryComponents(banner(TERMS_BANNER.en))
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`🇬🇧 ${strings.en.terms.body}`))
    .addSeparatorComponents(small())
    .addActionRowComponents(
      new ActionRowBuilder<MessageActionRowComponentBuilder>().addComponents(
        link(`${SITE}/terms`, "Regulamin", "🇵🇱"),
        link(`${SITE}/en/terms`, "Terms", "🇬🇧"),
        link(`${SITE}/privacy`, "Prywatność", "🇵🇱"),
        link(`${SITE}/en/privacy`, "Privacy", "🇬🇧"),
      ),
    )
    .addSeparatorComponents(small())
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`-# Opublikowano · Published: ${PUBLISHED_ISO}`));
}

/**
 * The about panel for the welcome channel - one bilingual card, with each
 * language's banner sitting full-width above its own copy, then the shared
 * (language-neutral) version and download count and the site links.
 *
 * Built as a container directly rather than through `panel()` because it
 * interleaves media and text per language, which the flat helper doesn't do.
 */
export async function buildAboutPanel() {
  const [status, installations] = await Promise.all([getStatus(), getInstallations()]);
  const installsLine =
    (installations.daily !== null ? `\n**Aktywne wczoraj · Active yesterday:** **${installations.daily.toLocaleString("pl-PL")}**` : "") +
    (installations.weekly !== null ? `\n**Aktywne w tygodniu · Active last week:** **${installations.weekly.toLocaleString("pl-PL")}**` : "");
  const small = () => new SeparatorBuilder().setSpacing(SeparatorSpacingSize.Small);
  const banner = (url: string) => new MediaGalleryBuilder().addItems(new MediaGalleryItemBuilder().setURL(normalizeImageUrl(url)));

  return new ContainerBuilder()
    .setAccentColor(COLOR.accent)
    .addTextDisplayComponents(new TextDisplayBuilder().setContent("## ✨ O aplikacji · About VibeSSH"))
    .addMediaGalleryComponents(banner(ABOUT_BANNER.pl))
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`🇵🇱 ${strings.pl.about.body}`))
    .addSeparatorComponents(small())
    .addMediaGalleryComponents(banner(ABOUT_BANNER.en))
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`🇬🇧 ${strings.en.about.body}`))
    .addSeparatorComponents(small())
    .addTextDisplayComponents(
      new TextDisplayBuilder().setContent(
        `**Wersja · Version:** \`${status.version}\`\n**Pobrania · Downloads:** **${status.downloads.toLocaleString("pl-PL")}**${installsLine}`,
      ),
    )
    .addActionRowComponents(
      new ActionRowBuilder<MessageActionRowComponentBuilder>().addComponents(
        link(SITE, "Strona · Website", "🌐"),
        link(`${SITE}/download`, "Pobierz · Download", "📥"),
      ),
    )
    .addSeparatorComponents(small())
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`-# Zaktualizowano · Updated: ${PUBLISHED_ISO}`));
}
