import {
  ChannelType,
  PermissionFlagsBits,
  PermissionsBitField,
  type Guild,
  type Role,
  type GuildBasedChannel,
  type CategoryChannel,
  type OverwriteResolvable,
} from "discord.js";
import { blueprint, TEAM_ROLE_KEYS, languageRoleKey, type Visibility, type ChannelSpec, type CategorySpec, type RoleSpec } from "./blueprint.js";
import { loadConfig, saveConfig, type RuntimeConfig } from "../store.js";
import { log } from "../lib/logger.js";

const REASON = "VibeSSH self-setup";

/** A comparable form of a channel/role name: lowercased, Polish diacritics
 *  folded, emoji and punctuation dropped - so "📢-ogłoszenia" and "ogloszenia"
 *  match the same existing channel and setup adopts it instead of duplicating. */
function slug(name: string): string {
  return name
    .toLowerCase()
    .replace(/[ąàáâ]/g, "a")
    .replace(/ć/g, "c")
    .replace(/[ęèéê]/g, "e")
    .replace(/ł/g, "l")
    .replace(/ń/g, "n")
    .replace(/[óòôø]/g, "o")
    .replace(/ś/g, "s")
    .replace(/[żź]/g, "z")
    .replace(/[^a-z0-9]+/g, "");
}

export interface SetupSummary {
  createdRoles: string[];
  adoptedRoles: string[];
  createdChannels: string[];
  adoptedChannels: string[];
}

/**
 * Makes the guild match the blueprint, and records which snowflake is which.
 *
 * Idempotent: run it on a fresh server and it builds everything; run it on the
 * server that already exists (the common case here) and it adopts the roles and
 * channels already there by name, stores their ids, and only fixes their
 * permissions. Never deletes anything - a channel the blueprint does not
 * mention is left alone.
 */
export async function reconcile(guild: Guild): Promise<SetupSummary> {
  const config = loadConfig();
  const summary: SetupSummary = { createdRoles: [], adoptedRoles: [], createdChannels: [], adoptedChannels: [] };

  await guild.roles.fetch();
  for (const spec of blueprint.roles) {
    await ensureRole(guild, spec, config, summary);
  }

  await guild.channels.fetch();
  for (const category of blueprint.categories) {
    const cat = await ensureCategory(guild, category, config, summary);
    for (const channel of category.channels) {
      await ensureChannel(guild, channel, category.visibility, cat, config, summary);
    }
  }

  saveConfig(config);
  return summary;
}

async function ensureRole(guild: Guild, spec: RoleSpec, config: RuntimeConfig, summary: SetupSummary): Promise<Role> {
  const permissions = new PermissionsBitField(spec.permissions ?? []);
  let role = trackedRole(guild, config.roles[spec.key]) ?? guild.roles.cache.find((r) => r.id !== guild.id && slug(r.name) === slug(spec.name));

  if (role) {
    // Only touch what drifted, so re-running setup is quiet in the audit log.
    if (role.name !== spec.name || role.hexColor !== `#${spec.color.toString(16).padStart(6, "0")}` || role.hoist !== Boolean(spec.hoist) || role.mentionable !== Boolean(spec.mentionable) || !role.permissions.equals(permissions)) {
      role = await role.edit({ name: spec.name, color: spec.color, hoist: Boolean(spec.hoist), mentionable: Boolean(spec.mentionable), permissions, reason: REASON });
    }
    summary.adoptedRoles.push(spec.name);
  } else {
    role = await guild.roles.create({ name: spec.name, color: spec.color, hoist: Boolean(spec.hoist), mentionable: Boolean(spec.mentionable), permissions, reason: REASON });
    summary.createdRoles.push(spec.name);
  }
  config.roles[spec.key] = role.id;
  return role;
}

async function ensureCategory(guild: Guild, spec: CategorySpec, config: RuntimeConfig, summary: SetupSummary): Promise<CategoryChannel> {
  const overwrites = categoryOverwrites(guild, spec.visibility, config);
  let cat = trackedChannel(guild, config.categories[spec.key]);
  if (!cat || cat.type !== ChannelType.GuildCategory) {
    cat = guild.channels.cache.find((c) => c.type === ChannelType.GuildCategory && slug(c.name) === slug(spec.name)) ?? undefined;
  }

  if (cat && cat.type === ChannelType.GuildCategory) {
    await cat.permissionOverwrites.set(overwrites, REASON);
    summary.adoptedChannels.push(spec.name);
  } else {
    cat = await guild.channels.create({ name: spec.name, type: ChannelType.GuildCategory, permissionOverwrites: overwrites, reason: REASON });
    summary.createdChannels.push(spec.name);
  }
  config.categories[spec.key] = cat.id;
  return cat as CategoryChannel;
}

async function ensureChannel(
  guild: Guild,
  spec: ChannelSpec,
  visibility: Visibility,
  category: CategoryChannel,
  config: RuntimeConfig,
  summary: SetupSummary,
): Promise<GuildBasedChannel> {
  const type = spec.kind === "voice" ? ChannelType.GuildVoice : spec.kind === "announcement" ? ChannelType.GuildAnnouncement : ChannelType.GuildText;

  // Every channel's overwrites are set explicitly to the category's, so
  // visibility never depends on Discord's category-sync being intact. A status
  // voice channel is public to see but not to join.
  const overwrites: OverwriteResolvable[] = [...categoryOverwrites(guild, visibility, config)];
  if (spec.status) {
    overwrites.push({ id: guild.id, deny: [PermissionFlagsBits.Connect] });
  }

  let channel = trackedChannel(guild, config.channels[spec.key]);
  if (!channel) {
    channel =
      guild.channels.cache.find((c) => c.parentId === category.id && slug(c.name) === slug(spec.name)) ??
      guild.channels.cache.find((c) => slug(c.name) === slug(spec.name) && sameChannelFamily(c.type, type)) ??
      undefined;
  }

  if (channel) {
    if (channel.parentId !== category.id && "setParent" in channel) {
      await channel.setParent(category.id, { lockPermissions: false, reason: REASON }).catch(() => undefined);
    }
    if ("permissionOverwrites" in channel) {
      await channel.permissionOverwrites.set(overwrites, REASON).catch(() => undefined);
    }
    summary.adoptedChannels.push(spec.name);
  } else {
    channel = await guild.channels.create({ name: spec.name, type, parent: category.id, topic: spec.topic, permissionOverwrites: overwrites, reason: REASON });
    summary.createdChannels.push(spec.name);
  }
  config.channels[spec.key] = channel.id;
  return channel;
}

/** Text and announcement channels are interchangeable enough to adopt across;
 *  voice must match voice. */
function sameChannelFamily(a: ChannelType, b: ChannelType): boolean {
  const textLike = new Set<ChannelType>([ChannelType.GuildText, ChannelType.GuildAnnouncement]);
  if (b === ChannelType.GuildVoice) return a === ChannelType.GuildVoice;
  return textLike.has(a);
}

function categoryOverwrites(guild: Guild, visibility: Visibility, config: RuntimeConfig): OverwriteResolvable[] {
  const everyone = guild.id;
  const view = PermissionFlagsBits.ViewChannel;
  const teamRoleIds = TEAM_ROLE_KEYS.map((key) => config.roles[key]).filter((id): id is string => Boolean(id));

  if (visibility.kind === "public") {
    return [{ id: everyone, allow: [view] }];
  }
  if (visibility.kind === "language") {
    const langRoleId = config.roles[languageRoleKey(visibility.lang)];
    return [
      { id: everyone, deny: [view] },
      ...(langRoleId ? [{ id: langRoleId, allow: [view] }] : []),
      ...teamRoleIds.map((id) => ({ id, allow: [view] })),
    ];
  }
  return [{ id: everyone, deny: [view] }, ...teamRoleIds.map((id) => ({ id, allow: [view] }))];
}

function trackedRole(guild: Guild, id: string | undefined): Role | undefined {
  return id ? (guild.roles.cache.get(id) ?? undefined) : undefined;
}

function trackedChannel(guild: Guild, id: string | undefined): GuildBasedChannel | undefined {
  return id ? (guild.channels.cache.get(id) ?? undefined) : undefined;
}

/** A one-line, human-readable recap for the /setup reply. */
export function summarize(summary: SetupSummary): string {
  const lines: string[] = [];
  const section = (label: string, created: string[], adopted: string[]) => {
    if (created.length) lines.push(`**${label} utworzone:** ${created.join(", ")}`);
    if (adopted.length) lines.push(`**${label} skonfigurowane:** ${adopted.join(", ")}`);
  };
  section("Role", summary.createdRoles, summary.adoptedRoles);
  section("Kanały", summary.createdChannels, summary.adoptedChannels);
  return lines.join("\n") || "Nic do zmiany - wszystko już na miejscu.";
}
