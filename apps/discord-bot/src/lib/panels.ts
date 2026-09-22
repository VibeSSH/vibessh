import type { ContainerBuilder, GuildTextBasedChannel } from "discord.js";
import { ids } from "../store.js";
import { editPanel, sendPanel } from "./reply.js";

/**
 * Posts a panel once and edits it thereafter.
 *
 * The picker, the ticket panels and the info/terms panels are single standing
 * messages, not a fresh post every time setup runs - so their message ids are
 * remembered (`config.runtime.json`) and re-running setup edits the existing
 * message in place. A message that has since been deleted falls back to a new
 * post, whose id is then remembered.
 */
export async function upsertPanel(channel: GuildTextBasedChannel, key: string, container: ContainerBuilder): Promise<void> {
  const existingId = ids.message(key);
  if (existingId) {
    const existing = await channel.messages.fetch(existingId).catch(() => null);
    if (existing) {
      await editPanel(existing, container);
      return;
    }
  }
  const sent = await sendPanel(channel, container);
  if (sent) ids.setMessage(key, sent.id);
}
