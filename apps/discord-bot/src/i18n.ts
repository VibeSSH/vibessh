/**
 * Every user-facing string, in both languages VibeSSH's community uses.
 *
 * The bot posts separate panels per language (a Polish embed in the Polish
 * category, an English one in the English category), so the copy lives here
 * keyed by language rather than being translated on the fly. `Strings` is the
 * shape both must fill, so a key added to one and forgotten in the other is a
 * type error, not a half-translated panel someone finds in production.
 */
export type Lang = "pl" | "en";
export const LANGS: readonly Lang[] = ["pl", "en"] as const;

export interface Strings {
  languagePanel: {
    title: string;
    body: string;
    /** Button label for choosing THIS language. */
    choose: string;
    /** Confirmation shown (ephemerally) after choosing. */
    chosen: string;
  };
  ticketPanel: {
    title: string;
    body: string;
    button: string;
    createdTitle: string;
    createdBody: string;
    close: string;
    closed: string;
    alreadyOpen: string;
  };
  terms: {
    title: string;
    body: string;
    footer: (publishedIso: string) => string;
  };
  about: {
    title: string;
    body: string;
    version: string;
    downloads: string;
    footer: (publishedIso: string) => string;
  };
  moderation: {
    linkBlocked: string;
    spamBlocked: string;
    gifOnlyInChat: string;
    banned: (reason: string) => string;
  };
  release: {
    heading: string;
    body: string;
    download: string;
  };
}

const pl: Strings = {
  languagePanel: {
    title: "Wybierz język / Choose your language",
    body: "Kliknij **Polski**, żeby odblokować polskie kanały. Angielskie zostaną ukryte - w każdej chwili możesz przełączyć.",
    choose: "Polski",
    chosen: "Ustawiono język na polski. Polskie kanały są już widoczne, angielskie ukryte.",
  },
  ticketPanel: {
    title: "Pomoc / Zgłoszenia",
    body: "Masz problem albo pytanie? Kliknij niżej, a otworzymy dla Ciebie prywatny kanał z zespołem.",
    button: "Otwórz zgłoszenie",
    createdTitle: "Zgłoszenie otwarte",
    createdBody: "Opisz swój problem - ktoś z zespołu zaraz się odezwie. Gdy skończysz, użyj przycisku niżej.",
    close: "Zamknij zgłoszenie",
    closed: "Zgłoszenie zostało zamknięte.",
    alreadyOpen: "Masz już otwarte zgłoszenie: {channel}.",
  },
  terms: {
    title: "Regulamin korzystania z VibeSSH",
    body: "Korzystając z VibeSSH oraz tego serwera, akceptujesz poniższe zasady. Pełny regulamin znajdziesz na vibessh.dev.",
    footer: (iso) => `Opublikowano: ${iso}`,
  },
  about: {
    title: "O aplikacji VibeSSH",
    body: "VibeSSH to aplikacja do zarządzania serwerami i aplikacjami przez SSH, z naciskiem na bezpieczeństwo i prostotę.",
    version: "Aktualna wersja",
    downloads: "Pobrania",
    footer: (iso) => `Zaktualizowano: ${iso}`,
  },
  moderation: {
    linkBlocked: "Twoja wiadomość zawierała link i została usunięta. Linki są dozwolone tylko dla zespołu.",
    spamBlocked: "Zwolnij trochę - wysyłasz wiadomości zbyt szybko.",
    gifOnlyInChat: "GIF-y i naklejki można wysyłać tylko na kanale czatowym.",
    banned: (reason) => `Zostałeś zbanowany. Powód: ${reason}`,
  },
  release: {
    heading: "Nowa wersja VibeSSH",
    body: "Jest nowa wersja! Pobierz ją poniżej albo zaktualizuj się w aplikacji.",
    download: "Pobierz",
  },
};

const en: Strings = {
  languagePanel: {
    title: "Choose your language / Wybierz język",
    body: "Click **English** to unlock the English channels. The Polish ones will be hidden - you can switch back any time.",
    choose: "English",
    chosen: "Language set to English. The English channels are now visible and the Polish ones are hidden.",
  },
  ticketPanel: {
    title: "Support / Tickets",
    body: "Got a problem or a question? Click below and we'll open a private channel with the team for you.",
    button: "Open a ticket",
    createdTitle: "Ticket opened",
    createdBody: "Describe your problem - someone from the team will be with you shortly. Use the button below when you're done.",
    close: "Close ticket",
    closed: "This ticket has been closed.",
    alreadyOpen: "You already have a ticket open: {channel}.",
  },
  terms: {
    title: "VibeSSH Terms of Use",
    body: "By using VibeSSH and this server you accept the rules below. The full terms are on vibessh.dev.",
    footer: (iso) => `Published: ${iso}`,
  },
  about: {
    title: "About VibeSSH",
    body: "VibeSSH is an app for managing servers and applications over SSH, built around security and simplicity.",
    version: "Current version",
    downloads: "Downloads",
    footer: (iso) => `Updated: ${iso}`,
  },
  moderation: {
    linkBlocked: "Your message contained a link and was removed. Links are allowed for the team only.",
    spamBlocked: "Slow down - you're sending messages too fast.",
    gifOnlyInChat: "GIFs and stickers can only be sent in the chat channel.",
    banned: (reason) => `You have been banned. Reason: ${reason}`,
  },
  release: {
    heading: "New VibeSSH release",
    body: "A new version is out! Download it below or update from inside the app.",
    download: "Download",
  },
};

export const strings: Record<Lang, Strings> = { pl, en };

export function t(lang: Lang): Strings {
  return strings[lang];
}
