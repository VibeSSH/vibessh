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
  mariadb: {
    name: "MariaDB",
    description:
      "Samodzielny serwer bazy MariaDB - dane leżą we własnym katalogu roboczym tej aplikacji. Zanim go uruchomisz, ustaw MYSQL_ROOT_PASSWORD (oraz ewentualnie MYSQL_DATABASE/MYSQL_USER/MYSQL_PASSWORD, jeśli chcesz je utworzyć od razu) w zakładce Środowisko.",
    fields: {
      mariadbVersion: { label: "Wersja MariaDB", helpText: "Tag z Docker Huba, np. 11, 10.11, 10.6 albo lts." },
    },
  },
  redis: {
    name: "Redis",
    description: "Samodzielna instancja Redisa z trwałością append-only - dane leżą we własnym katalogu roboczym tej aplikacji.",
    fields: {
      redisVersion: { label: "Wersja Redisa", helpText: "Tag z Docker Huba, np. 7, 8 albo alpine." },
      requirePassword: {
        label: "Hasło",
        helpText:
          "Ustawia --requirepass. Puste znaczy bez uwierzytelniania - bezpieczne tylko wtedy, gdy ten port pozostaje prywatny (zobacz widoczność w zakładce Porty).",
      },
    },
  },
  phpmyadmin: {
    name: "phpMyAdmin",
    description:
      "Webowy interfejs do zarządzania serwerem MySQL/MariaDB - wskaż mu dowolny osiągalny serwer (host bazy albo aplikację MariaDB w Vibe Network), ustawiając PMA_HOST (i PMA_PORT, jeśli inny niż 3306) w zakładce Środowisko.",
    fields: {
      phpMyAdminVersion: { label: "Wersja phpMyAdmin", helpText: "Tag z Docker Huba, np. latest albo konkretna wersja." },
    },
  },
  "nodejs-bot": {
    name: "Bot Node.js",
    description: "Uruchamia skrypt lub bota w Node.js - przed każdym startem instaluje zależności npm z package.json, jeśli plik istnieje.",
    fields: {
      entryFile: { label: "Plik wejściowy", helpText: "Ścieżka do skryptu względem katalogu roboczego, np. index.js albo src/bot.js." },
      nodeVersion: { label: "Wersja Node.js", helpText: "Tag z Docker Huba, np. 22, 20 albo 18." },
      programArgs: { label: "Argumenty programu", helpText: "Argumenty przekazywane samemu skryptowi." },
    },
  },
  "python-bot": {
    name: "Bot Python",
    description: "Uruchamia skrypt lub bota w Pythonie - przed każdym startem instaluje zależności pip z requirements.txt, jeśli plik istnieje.",
    fields: {
      entryFile: { label: "Plik wejściowy", helpText: "Ścieżka do skryptu względem katalogu roboczego, np. bot.py albo src/main.py." },
      pythonVersion: { label: "Wersja Pythona", helpText: "Tag z Docker Huba, np. 3.13, 3.12 albo 3.11." },
      programArgs: { label: "Argumenty programu", helpText: "Argumenty przekazywane samemu skryptowi." },
    },
  },
  purpur: {
    name: "Purpur",
    description: "Fork Papera z dodatkowymi opcjami wydajności i rozgrywki - plik jar serwera jest pobierany i aktualizowany automatycznie.",
    fields: {
      purpurVersion: { label: "Wersja Minecrafta", helpText: "Pasujący plik jar serwera Purpur zostanie pobrany automatycznie." },
      eulaAccepted: { label: "Akceptuję EULA Minecrafta (https://www.minecraft.net/eula)" },
      javaVersion: {
        label: "Wersja Javy",
        helpText: "Dowolna główna wersja Javy dostępna jako obraz eclipse-temurin, np. 25, 21, 17 albo 11 - wybiera pasujący obraz Dockera.",
      },
      jvmArgs: { label: "Argumenty JVM", helpText: "Flagi przekazywane samej maszynie wirtualnej, przed -jar - np. -Xmx2G." },
      programArgs: { label: "Argumenty programu", helpText: "Argumenty przekazywane serwerowi." },
    },
  },
  waterfall: {
    name: "Waterfall",
    description: "Proxy Minecrafta oparte na BungeeCordzie - plik jar jest pobierany i aktualizowany automatycznie.",
    fields: {
      waterfallVersion: { label: "Wersja Waterfalla", helpText: "Pasujący plik jar zostanie pobrany automatycznie." },
      javaVersion: {
        label: "Wersja Javy",
        helpText: "Dowolna główna wersja Javy dostępna jako obraz eclipse-temurin, np. 25, 21, 17 albo 11 - wybiera pasujący obraz Dockera.",
      },
      jvmArgs: { label: "Argumenty JVM", helpText: "Flagi przekazywane samej maszynie wirtualnej, przed -jar - np. -Xmx2G." },
      programArgs: { label: "Argumenty programu", helpText: "Argumenty przekazywane proxy." },
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
