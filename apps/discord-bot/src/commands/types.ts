import type { ChatInputCommandInteraction, SlashCommandOptionsOnlyBuilder, SlashCommandBuilder, SlashCommandSubcommandsOnlyBuilder } from "discord.js";

export interface BotCommand {
  data: SlashCommandBuilder | SlashCommandOptionsOnlyBuilder | SlashCommandSubcommandsOnlyBuilder;
  execute(interaction: ChatInputCommandInteraction): Promise<void>;
}
