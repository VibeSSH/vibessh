import { PermissionFlagsBits } from "discord.js";
import { COLOR } from "../lib/theme.js";
import type { Lang } from "../i18n.js";

/**
 * The server VibeSSH wants, described as data.
 *
 * The setup routine (`reconcile`) reads this and makes the real server match
 * it - creating a role or channel that is missing, adopting one that already
 * exists by name, and applying the permissions each category implies. Nothing
 * about "which snowflake" lives here; that is discovered and written to
 * `config.runtime.json`. Editing this file and re-running `/setup` is how the
 * layout changes, so the structure stays reviewable in one place instead of
 * being clicked into Discord by hand and forgotten.
 */

/** Who can see a category (and therefore its channels). */
export type Visibility =
  | { kind: "public" }
  | { kind: "language"; lang: Lang }
  | { kind: "team" };

export type ChannelKind = "text" | "voice" | "announcement";

export interface ChannelSpec {
  key: string;
  name: string;
  kind?: ChannelKind;
  topic?: string;
  /** The one channel per language where gifs/stickers/links are tolerated. */
  chatChannel?: boolean;
  /** Members may post here. Everything without this (and without `chatChannel`)
   * is read-only for `@everyone` - welcome, rules, releases and announcements
   * are things the team and the bot post, not things members write into. Staff
   * roles keep the ability to post everywhere regardless. */
  writable?: boolean;
  /** Post the language picker here (the `choose-language` channel). */
  languagePanel?: boolean;
  /** Post a ticket panel for this language here. */
  ticketPanel?: Lang;
  /** A read-only voice channel whose name the bot keeps updated with live stats. */
  status?: "downloads" | "version";
}

export interface CategorySpec {
  key: string;
  name: string;
  visibility: Visibility;
  channels: ChannelSpec[];
}

export interface RoleSpec {
  key: string;
  name: string;
  color: number;
  hoist?: boolean;
  mentionable?: boolean;
  /** Guild-wide permissions to grant (team roles). */
  permissions?: bigint[];
  /** Set when this is a language role the picker toggles. */
  language?: Lang;
  /** Counts as staff: sees the TEAM category, ignored by moderation filters. */
  team?: boolean;
}

export interface Blueprint {
  roles: RoleSpec[];
  categories: CategorySpec[];
}

export const blueprint: Blueprint = {
  roles: [
    { key: "founder", name: "Founder", color: COLOR.danger, hoist: true, team: true, permissions: [PermissionFlagsBits.Administrator] },
    {
      key: "developer",
      name: "Developer",
      color: 0xa855f7,
      hoist: true,
      team: true,
      permissions: [
        PermissionFlagsBits.ManageGuild,
        PermissionFlagsBits.ManageChannels,
        PermissionFlagsBits.ManageRoles,
        PermissionFlagsBits.ManageMessages,
        PermissionFlagsBits.KickMembers,
        PermissionFlagsBits.BanMembers,
      ],
    },
    {
      key: "moderator",
      name: "Moderator",
      color: COLOR.success,
      hoist: true,
      team: true,
      permissions: [
        PermissionFlagsBits.KickMembers,
        PermissionFlagsBits.BanMembers,
        PermissionFlagsBits.ManageMessages,
        PermissionFlagsBits.ModerateMembers,
        PermissionFlagsBits.ManageThreads,
      ],
    },
    {
      key: "support",
      name: "Support",
      color: 0x2563eb,
      hoist: true,
      team: true,
      permissions: [PermissionFlagsBits.ManageMessages, PermissionFlagsBits.ModerateMembers, PermissionFlagsBits.ManageThreads],
    },
    { key: "contributor", name: "Contributor", color: 0xf97316, hoist: true },
    { key: "beta-tester", name: "Beta Tester", color: 0xeab308, hoist: true },
    { key: "lang-pl", name: "Polski", color: COLOR.neutral, language: "pl" },
    { key: "lang-en", name: "English", color: COLOR.neutral, language: "en" },
    { key: "notify-release", name: "Release Notifications", color: COLOR.neutral, mentionable: true },
    { key: "notify-testing", name: "Testing Notifications", color: COLOR.neutral, mentionable: true },
  ],
  categories: [
    {
      key: "start-here",
      name: "👋 START HERE",
      visibility: { kind: "public" },
      channels: [
        { key: "welcome", name: "👋-welcome", topic: "Welcome to VibeSSH." },
        { key: "rules", name: "📜-rules" },
        { key: "choose-language", name: "🌍-choose-language", languagePanel: true, topic: "Pick a language to unlock its channels." },
        { key: "releases", name: "🚀-releases", kind: "announcement", topic: "New VibeSSH releases." },
      ],
    },
    {
      key: "status",
      name: "📊 STATUS",
      visibility: { kind: "public" },
      channels: [
        { key: "status-version", name: "🏷️ Wersja: ...", kind: "voice", status: "version" },
        { key: "status-downloads", name: "📥 Pobrania: ...", kind: "voice", status: "downloads" },
      ],
    },
    {
      key: "polski",
      name: "🇵🇱 POLSKI",
      visibility: { kind: "language", lang: "pl" },
      channels: [
        { key: "pl-announcements", name: "📢-ogłoszenia", kind: "announcement" },
        { key: "pl-general", name: "💬-pogadanki", chatChannel: true },
        { key: "pl-support", name: "🆘-pomoc", ticketPanel: "pl" },
        { key: "pl-suggestions", name: "💡-propozycje", writable: true },
      ],
    },
    {
      key: "english",
      name: "🇬🇧 ENGLISH",
      visibility: { kind: "language", lang: "en" },
      channels: [
        { key: "en-announcements", name: "📢-announcements", kind: "announcement" },
        { key: "en-general", name: "💬-general", chatChannel: true },
        { key: "en-support", name: "🆘-support", ticketPanel: "en" },
        { key: "en-suggestions", name: "💡-suggestions", writable: true },
      ],
    },
    {
      key: "team",
      name: "🔒 TEAM",
      visibility: { kind: "team" },
      channels: [
        { key: "staff-chat", name: "💬-staff-chat" },
        { key: "reports", name: "📋-reports" },
        { key: "logs", name: "🗄️-logs" },
        { key: "dev-logs", name: "🛠️-dev-logs", topic: "GitHub commits and pull requests." },
      ],
    },
  ],
};

/** The role keys that count as staff, derived once from the blueprint. */
export const TEAM_ROLE_KEYS = blueprint.roles.filter((role) => role.team).map((role) => role.key);

/** The language role key for a language. */
export function languageRoleKey(lang: Lang): string {
  const role = blueprint.roles.find((r) => r.language === lang);
  if (!role) throw new Error(`no language role defined for ${lang}`);
  return role.key;
}
