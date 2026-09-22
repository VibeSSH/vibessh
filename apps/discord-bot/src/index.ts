import { Client, GatewayIntentBits, Events, MessageFlags, type Interaction } from "discord.js";
import { env } from "./env.js";
import { log } from "./lib/logger.js";
import { commandMap } from "./commands/index.js";
import { handleLanguageButton, isLanguageButton } from "./features/language.js";
import { handleTicketButton, isTicketButton } from "./features/tickets.js";
import { handleNotifyButton, isNotifyButton } from "./features/notifications.js";
import { moderateMessage } from "./features/moderation.js";
import { startStatusUpdater } from "./features/statusChannels.js";
import { startReleaseWatcher } from "./features/releaseWatcher.js";
import { startPresence } from "./features/presence.js";
import { handleMemberJoin, handleMemberLeave } from "./features/memberLog.js";

/**
 * The bot process.
 *
 * `Guilds` covers channels and roles, `GuildMembers` hands out the language
 * role, and `GuildMessages` + `MessageContent` let the moderation filter read
 * message text. `MessageContent` is privileged: it has to be enabled under
 * Bot -> Privileged Gateway Intents in the Developer Portal, next to Server
 * Members, or login fails with a disallowed-intents error.
 */
const client = new Client({
  intents: [GatewayIntentBits.Guilds, GatewayIntentBits.GuildMembers, GatewayIntentBits.GuildMessages, GatewayIntentBits.MessageContent],
});

/** The bot's profile description ("About Me"). Set on boot via the application
 *  API rather than by hand in the Developer Portal, so it stays in one place. */
const DESCRIPTION = [
  "🇵🇱 Oficjalny bot społeczności VibeSSH - konfiguruje serwer, wybór języka, tickety, moderację i powiadomienia o wydaniach.",
  "🇬🇧 The official VibeSSH community bot - server setup, language selection, tickets, moderation and release notifications.",
  "🌐 vibessh.dev",
].join("\n");

client.once(Events.ClientReady, (ready) => {
  log.ok(`Logged in as ${ready.user.tag} - serving ${ready.guilds.cache.size} guild(s).`);
  void ready.application
    .edit({ description: DESCRIPTION })
    .catch((error) => log.warn(`couldn't set bot description: ${error instanceof Error ? error.message : String(error)}`));
  startStatusUpdater(ready);
  startReleaseWatcher(ready);
  startPresence(ready);
});

client.on(Events.MessageCreate, (message) => {
  void moderateMessage(message);
});

client.on(Events.GuildMemberAdd, (member) => {
  void handleMemberJoin(member);
});

client.on(Events.GuildMemberRemove, (member) => {
  void handleMemberLeave(member);
});

client.on(Events.InteractionCreate, async (interaction: Interaction) => {
  try {
    if (interaction.isChatInputCommand()) {
      const command = commandMap.get(interaction.commandName);
      if (!command) return;
      await command.execute(interaction);
      return;
    }

    if (interaction.isButton()) {
      if (isLanguageButton(interaction.customId)) {
        await handleLanguageButton(interaction);
        return;
      }
      if (isTicketButton(interaction.customId)) {
        await handleTicketButton(interaction);
        return;
      }
      if (isNotifyButton(interaction.customId)) {
        await handleNotifyButton(interaction);
        return;
      }
    }
  } catch (error) {
    log.error(`interaction ${interaction.isCommand() ? interaction.commandName : "component"} failed`, error);
    if (interaction.isRepliable() && !interaction.replied && !interaction.deferred) {
      await interaction.reply({ content: "Coś poszło nie tak. / Something went wrong.", flags: MessageFlags.Ephemeral }).catch(() => undefined);
    }
  }
});

client.login(env.token).catch((error) => {
  log.error("Login failed - check DISCORD_TOKEN", error);
  process.exit(1);
});
