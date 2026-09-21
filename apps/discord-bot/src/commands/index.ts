import { Collection } from "discord.js";
import { setupCommand } from "./setup.js";
import { banCommand, unbanCommand } from "./ban.js";
import { releaseCommand } from "./release.js";
import { devlogsCommand } from "./devlogs.js";
import type { BotCommand } from "./types.js";

/** Every slash command the bot answers, in one list. Add a command here and it
 *  is both deployed (`deploy-commands`) and routed (`index`) with no further
 *  wiring. */
export const commands: BotCommand[] = [setupCommand, banCommand, unbanCommand, releaseCommand, devlogsCommand];

export const commandMap = new Collection<string, BotCommand>(commands.map((command) => [command.data.name, command]));
