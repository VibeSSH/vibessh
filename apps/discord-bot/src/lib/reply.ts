import {
  MessageFlags,
  type ContainerBuilder,
  type RepliableInteraction,
  type TextBasedChannel,
  type Message,
} from "discord.js";

/**
 * Sending a Components V2 message is the same three lines everywhere - the
 * `IsComponentsV2` flag, the container in `components`, and no `content` (the
 * two are mutually exclusive). These wrap that so a caller just hands over a
 * `panel(...)`.
 */

export async function replyPanel(
  interaction: RepliableInteraction,
  container: ContainerBuilder,
  options: { ephemeral?: boolean } = {},
): Promise<void> {
  const flags = options.ephemeral ? MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral : MessageFlags.IsComponentsV2;
  if (interaction.deferred || interaction.replied) {
    await interaction.followUp({ components: [container], flags });
  } else {
    await interaction.reply({ components: [container], flags });
  }
}

export async function sendPanel(channel: TextBasedChannel, container: ContainerBuilder): Promise<Message | undefined> {
  if (!channel.isSendable()) return undefined;
  return channel.send({ components: [container], flags: MessageFlags.IsComponentsV2 });
}

export async function editPanel(message: Message, container: ContainerBuilder): Promise<Message> {
  return message.edit({ components: [container], flags: MessageFlags.IsComponentsV2 });
}
