import type { Blueprint } from "@/types/application";

interface BlueprintFieldTranslation {
  label?: string;
  helpText?: string;
}

interface BlueprintTranslation {
  name?: string;
  description?: string;
  fields?: Record<string, BlueprintFieldTranslation>;
}

/**
 * A built-in Blueprint's name/description/field text comes straight from
 * the Rust backend as plain English (`src-tauri/src/blueprints/*.rs`) -
 * there's no backend-side i18n mechanism for it, and building one (locale
 * negotiation over the Tauri IPC boundary, translated copies of every
 * field baked into the Rust structs) would be a lot of backend surface for
 * five known, stable blueprint ids. This overlays a Polish translation for
 * VibeSSH's own built-ins instead, keyed by blueprint id and field key.
 * Anything not listed here (a community/custom blueprint someone builds)
 * is left exactly as its author wrote it - untranslated is more honest
 * than a guessed-at translation of content this app has never seen before.
 */
const PL_BLUEPRINT_TRANSLATIONS: Record<string, BlueprintTranslation> = {
  generic: {
    name: "Aplikacja ogólna",
    description: "Uruchamia dowolne polecenie, które podasz.",
    fields: {
      command: { label: "Polecenie", helpText: "Plik wykonywalny do uruchomienia, np. /usr/bin/python3" },
      args: { label: "Argumenty", helpText: "Argumenty przekazywane do polecenia, w kolejności." },
    },
  },
  "generic-docker": {
    name: "Kontener Docker (ogólny)",
    description: "Uruchamia dowolny obraz Dockera, opcjonalnie nadpisując jego komendę.",
    fields: {
      image: { label: "Obraz", helpText: "Referencja obrazu Dockera, np. nginx:latest lub itzg/minecraft-server:latest" },
      command: {
        label: "Nadpisanie komendy",
        helpText: "Nadpisuje własny ENTRYPOINT/CMD obrazu, jeden argument na wpis. Zostaw puste, aby uruchomić obraz tak, jak został zbudowany.",
      },
    },
  },
  "generic-java": {
    name: "Aplikacja Java (ogólna)",
    description: "Uruchamia plik .jar za pomocą JVM.",
    fields: {
      javaVersion: {
        label: "Wersja Javy",
        helpText: "Dowolna wersja główna Javy dostępna jako obraz eclipse-temurin, np. 25, 21, 17 lub 11 - wybiera odpowiedni obraz Dockera.",
      },
      jarPath: { label: "Plik jar", helpText: "Ścieżka do pliku .jar, względem katalogu roboczego aplikacji." },
      jvmArgs: { label: "Argumenty JVM", helpText: "Flagi przekazywane do samego JVM, przed -jar - np. -Xmx2G." },
      programArgs: { label: "Argumenty programu", helpText: "Argumenty przekazywane do samego jara, po jego własnym wpisie -jar." },
    },
  },
  paper: {
    description: "Wydajny serwer Minecraft - plik jar serwera jest pobierany i aktualizowany automatycznie.",
    fields: {
      minecraftVersion: { label: "Wersja Minecrafta", helpText: "Pasujący plik jar Papera zostanie pobrany automatycznie." },
      eulaAccepted: { label: "Akceptuję EULA Minecrafta (https://www.minecraft.net/eula)" },
      javaVersion: {
        label: "Wersja Javy",
        helpText: "Dowolna wersja główna Javy dostępna jako obraz eclipse-temurin, np. 25, 21, 17 lub 11 - wybiera odpowiedni obraz Dockera.",
      },
      jvmArgs: { label: "Argumenty JVM", helpText: "Flagi przekazywane do samego JVM, przed -jar - np. -Xmx2G." },
      programArgs: { label: "Argumenty programu", helpText: "Argumenty przekazywane do pliku jar serwera." },
    },
  },
  velocity: {
    description: "Proxy Minecraft - jar jest pobierany i aktualizowany automatycznie.",
    fields: {
      velocityVersion: { label: "Wersja Velocity", helpText: "Pasujący plik jar Velocity zostanie pobrany automatycznie." },
      javaVersion: {
        label: "Wersja Javy",
        helpText: "Dowolna wersja główna Javy dostępna jako obraz eclipse-temurin, np. 25, 21, 17 lub 11 - wybiera odpowiedni obraz Dockera.",
      },
      jvmArgs: { label: "Argumenty JVM", helpText: "Flagi przekazywane do samego JVM, przed -jar - np. -Xmx1G." },
      programArgs: { label: "Argumenty programu", helpText: "Argumenty przekazywane do samego jara proxy." },
    },
  },
};

/** Applies the Polish overlay above to a Blueprint fetched from the backend - a no-op for any other language, or for a blueprint id this table doesn't recognize. */
export function translateBlueprint(blueprint: Blueprint, language: string): Blueprint {
  if (!language.startsWith("pl")) return blueprint;
  const translation = PL_BLUEPRINT_TRANSLATIONS[blueprint.id];
  if (!translation) return blueprint;
  return {
    ...blueprint,
    name: translation.name ?? blueprint.name,
    description: translation.description ?? blueprint.description,
    fields: blueprint.fields.map((field) => {
      const fieldTranslation = translation.fields?.[field.key];
      if (!fieldTranslation) return field;
      return {
        ...field,
        label: fieldTranslation.label ?? field.label,
        helpText: fieldTranslation.helpText ?? field.helpText,
      };
    }),
  };
}
