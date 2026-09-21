import {
  ContainerBuilder,
  TextDisplayBuilder,
  SectionBuilder,
  SeparatorBuilder,
  SeparatorSpacingSize,
  MediaGalleryBuilder,
  MediaGalleryItemBuilder,
  ThumbnailBuilder,
  ActionRowBuilder,
  type ButtonBuilder,
  type MessageActionRowComponentBuilder,
} from "discord.js";

/**
 * Turns whatever image link someone pasted into one Discord will actually
 * render.
 *
 * Imgur is the common case: people copy the *page* url (`imgur.com/AbC1d`),
 * which is an HTML page, not an image, and Discord shows nothing. The direct
 * file (`i.imgur.com/AbC1d.png`) is what a media component needs. A link that
 * is already a direct file, or lives anywhere else, is left untouched.
 */
export function normalizeImageUrl(url: string): string {
  const trimmed = url.trim();
  const imgurPage = /^https?:\/\/(?:www\.)?imgur\.com\/(?!gallery\/|a\/)([A-Za-z0-9]+)(?:\.[A-Za-z0-9]+)?$/i.exec(trimmed);
  if (imgurPage) {
    return `https://i.imgur.com/${imgurPage[1]}.png`;
  }
  return trimmed;
}

export interface PanelField {
  label: string;
  value: string;
}

export interface PanelOptions {
  /** Left accent stripe colour, e.g. 0x14b8a6 for the VibeSSH teal. */
  accent?: number;
  /** A full-width image at the very top of the panel, above the title - a
   *  banner. An imgur link (page or direct) is fine. */
  headerImage?: string;
  /** Bold heading line. */
  title?: string;
  /** Markdown body. Discord markdown, so **bold**, lists, links all work. */
  body?: string;
  /** A full-width image under the body - an imgur link is fine, page or direct. */
  image?: string;
  /** A small image floated beside the title/body instead of a full-width one. */
  thumbnail?: string;
  /** Label/value rows rendered as a compact block under the body. */
  fields?: PanelField[];
  /** A muted line at the very bottom - a publication date, a note, a version. */
  footer?: string;
  /** Buttons, laid out in rows of up to five. */
  buttons?: ButtonBuilder[];
}

/**
 * The one "rich message" builder the whole bot uses, on Components V2.
 *
 * Everything the community sees - the language picker, ticket panels, the terms
 * and info panels, moderation notices - is a `panel`, so they share one look
 * (accent stripe, heading, body, optional image, footer) and one place to
 * change it. Returned as a single `ContainerBuilder`; send it with
 * `sendPanel`/`replyPanel` below, which set the Components V2 flag for you.
 */
export function panel(options: PanelOptions): ContainerBuilder {
  const container = new ContainerBuilder();
  if (options.accent !== undefined) container.setAccentColor(options.accent);

  if (options.headerImage) {
    container.addMediaGalleryComponents(
      new MediaGalleryBuilder().addItems(new MediaGalleryItemBuilder().setURL(normalizeImageUrl(options.headerImage))),
    );
  }

  const heading = [options.title ? `## ${options.title}` : "", options.body ?? ""].filter(Boolean).join("\n\n");

  // A thumbnail has to hang off a Section (its only accessory slot); without
  // one the heading is a plain full-width text display.
  if (options.thumbnail) {
    const section = new SectionBuilder()
      .addTextDisplayComponents(new TextDisplayBuilder().setContent(heading || "​"))
      .setThumbnailAccessory(new ThumbnailBuilder({ media: { url: normalizeImageUrl(options.thumbnail) } }));
    container.addSectionComponents(section);
  } else if (heading) {
    container.addTextDisplayComponents(new TextDisplayBuilder().setContent(heading));
  }

  if (options.fields && options.fields.length > 0) {
    const rendered = options.fields.map((field) => `**${field.label}**\n${field.value}`).join("\n\n");
    container.addSeparatorComponents(new SeparatorBuilder().setSpacing(SeparatorSpacingSize.Small));
    container.addTextDisplayComponents(new TextDisplayBuilder().setContent(rendered));
  }

  if (options.image) {
    container.addMediaGalleryComponents(
      new MediaGalleryBuilder().addItems(new MediaGalleryItemBuilder().setURL(normalizeImageUrl(options.image))),
    );
  }

  if (options.buttons && options.buttons.length > 0) {
    for (let i = 0; i < options.buttons.length; i += 5) {
      const row = new ActionRowBuilder<MessageActionRowComponentBuilder>().addComponents(...options.buttons.slice(i, i + 5));
      container.addActionRowComponents(row);
    }
  }

  if (options.footer) {
    container.addSeparatorComponents(new SeparatorBuilder().setSpacing(SeparatorSpacingSize.Small));
    container.addTextDisplayComponents(new TextDisplayBuilder().setContent(`-# ${options.footer}`));
  }

  return container;
}
