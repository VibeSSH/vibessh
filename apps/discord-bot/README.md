# VibeSSH Discord bot

The community bot for the VibeSSH Discord. It configures itself - roles,
channels and permissions - from a single blueprint, drives language selection,
tickets, moderation and a live status, all on Discord.js v14 **Components V2**
(no legacy embeds).

## Stack

- **discord.js v14** with Components V2 (`ContainerBuilder`, `SectionBuilder`,
  `MediaGalleryBuilder`, …). Every rich message is built by `src/lib/components.ts`'s
  `panel()`, which supports a full-width image or a thumbnail from any URL -
  including an imgur page link, which it rewrites to the direct file so Discord
  actually renders it.
- **TypeScript**, run with **bun** (`bun run` executes `.ts` directly).

## Setup

1. Create an application + bot at <https://discord.com/developers/applications>.
   - Under **Bot**, enable the **Server Members Intent** (the bot hands out the
     language role). The message-content intent is only needed once moderation
     lands.
   - Invite it with the `bot` and `applications.commands` scopes and, at minimum,
     **Manage Roles**, **Manage Channels**, **Ban Members**, **Moderate Members**,
     **Manage Messages**.
   - In **Server Settings → Roles**, drag the bot's role **above** every role it
     manages (it cannot assign a role sitting above its own).
2. `cp .env.example .env` and fill in `DISCORD_TOKEN`, `CLIENT_ID`, `GUILD_ID`.
   Optionally set `STATUS_URL` (see below).
3. `bun install`
4. `bun run deploy` - registers the slash commands to your guild (instant).
5. `bun run start` (or `bun run dev` to auto-restart on changes).
6. In Discord, run **`/setup`** once. It adopts the roles and channels you
   already have (matching by name), creates anything missing, and applies the
   per-language and team visibility. It is safe to re-run.

## How the pieces fit

- `src/setup/blueprint.ts` - the server as data: roles (name, colour,
  permissions), categories and channels, and who each category is visible to
  (`public`, `language: pl|en`, `team`). Change the layout here, re-run `/setup`.
- `src/setup/reconcile.ts` - makes the real server match the blueprint and
  records which Discord id is which in `config.runtime.json`. Never deletes.
- `src/features/language.ts` - the `choose-language` picker. A button hands out
  the language role; the category overwrites the setup applied do the rest, so
  picking Polish reveals the Polish category and hides the English one.
- `src/i18n.ts` - every user-facing string in **pl** and **en**; panels are
  posted per language.

## Live status (`STATUS_URL`)

The two voice channels under **STATUS** show the current version and download
count. The bot reads them from `STATUS_URL`, which must answer with JSON:

```json
{ "version": "0.1.0-beta.18", "downloads": 1234 }
```

Until that endpoint exists, leave `STATUS_URL` blank and the bot shows
`STATUS_FALLBACK_VERSION` / `STATUS_FALLBACK_DOWNLOADS`.

## Status of features

- [x] Self-configuring roles / channels / permissions (`/setup`)
- [x] Language selection → category visibility
- [x] Components V2 panel builder with image / imgur support
- [ ] Tickets (pl / en panels)
- [ ] Moderation: anti-spam, anti-link, gifs only in chat, ban system
- [ ] Live status voice channels (version + downloads)
- [ ] Info panels: terms of use, about, with publication date
