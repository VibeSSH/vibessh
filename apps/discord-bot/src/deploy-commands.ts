import { REST, Routes } from "discord.js";
import { env } from "./env.js";
import { commands } from "./commands/index.js";
import { log } from "./lib/logger.js";

/**
 * Registers the slash commands with Discord, scoped to the one guild.
 *
 * Guild commands appear instantly, unlike global ones which take up to an hour
 * to propagate - so this is safe to re-run on every deploy. Run it whenever the
 * command list or a command's options change: `bun run deploy`.
 */
async function main() {
  const body = commands.map((command) => command.data.toJSON());
  const rest = new REST({ version: "10" }).setToken(env.token);

  log.info(`Registering ${body.length} command(s) to guild ${env.guildId}...`);
  await rest.put(Routes.applicationGuildCommands(env.clientId, env.guildId), { body });
  log.ok("Commands registered.");
}

main().catch((error) => {
  log.error("Failed to register commands", error);
  process.exit(1);
});
