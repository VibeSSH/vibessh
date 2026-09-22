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
import { LANGS, strings, type Lang } from "../i18n.js";
import { languageRoleKey } from "../setup/blueprint.js";
import { log } from "../lib/logger.js";

const PICK_PREFIX = "lang:pick:";
/** The banner shown above each language's copy in the picker. */
const LANG_BANNER: Record<Lang, string> = {
  pl: "https://i.imgur.com/l2C8JAq.png",
  en: "https://i.imgur.com/Dm2WSEg.png",
};

/**
 * The one panel in `choose-language`: pick a language, get its role, and the
 * category for it appears while the other disappears - all driven by the role
 * the button hands out and the category overwrites the setup applied.
 *
 * Bilingual on purpose: this is the one place a visitor has not chosen a
 * language yet, so it speaks both.
 */
export function buildLanguagePanel() {
  const small = () => new SeparatorBuilder().setSpacing(SeparatorSpacingSize.Small);
  const banner = (url: string) => new MediaGalleryBuilder().addItems(new MediaGalleryItemBuilder().setURL(normalizeImageUrl(url)));
  const button = (lang: Lang) =>
    new ButtonBuilder()
      .setCustomId(`${PICK_PREFIX}${lang}`)
      .setLabel(strings[lang].languagePanel.choose)
      .setStyle(lang === "pl" ? ButtonStyle.Danger : ButtonStyle.Primary)
      .setEmoji(lang === "pl" ? "🇵🇱" : "🇬🇧");

  return new ContainerBuilder()
    .setAccentColor(COLOR.accent)
    .addTextDisplayComponents(new TextDisplayBuilder().setContent("## 🌍 Wybierz język · Choose your language"))
    .addMediaGalleryComponents(banner(LANG_BANNER.pl))
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`🇵🇱 ${strings.pl.languagePanel.body}`))
    .addSeparatorComponents(small())
    .addMediaGalleryComponents(banner(LANG_BANNER.en))
    .addTextDisplayComponents(new TextDisplayBuilder().setContent(`🇬🇧 ${strings.en.languagePanel.body}`))
    .addSeparatorComponents(small())
    .addActionRowComponents(new ActionRowBuilder<MessageActionRowComponentBuilder>().addComponents(button("pl"), button("en")));
}

export function isLanguageButton(customId: string): boolean {
  return customId.startsWith(PICK_PREFIX);
}

/** Applies the chosen language: adds its role, removes every other language
 *  role, so exactly one language category is ever visible at a time. */
export async function handleLanguageButton(interaction: ButtonInteraction): Promise<void> {
  const lang = interaction.customId.slice(PICK_PREFIX.length) as Lang;
  if (!LANGS.includes(lang) || !interaction.inCachedGuild()) return;

  const member = interaction.member as GuildMember;
  const chosenRoleId = ids.role(languageRoleKey(lang));
  if (!chosenRoleId) {
    await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Language roles are not set up yet - an admin needs to run `/setup` first." }), { ephemeral: true });
    return;
  }

  const otherRoleIds = LANGS.filter((other) => other !== lang)
    .map((other) => ids.role(languageRoleKey(other)))
    .filter((id): id is string => Boolean(id));

  try {
    await member.roles.remove(otherRoleIds, "Language switch");
    await member.roles.add(chosenRoleId, "Language selection");
    await replyPanel(interaction, panel({ accent: COLOR.success, body: strings[lang].languagePanel.chosen }), { ephemeral: true });
  } catch (error) {
    log.error(`failed to set language ${lang} for ${member.id}`, error);
    await replyPanel(interaction, panel({ accent: COLOR.danger, body: "Couldn't change your language - the bot's role may be below the language roles. Ping an admin." }), { ephemeral: true });
  }
}
