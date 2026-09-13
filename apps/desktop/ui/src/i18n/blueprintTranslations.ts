import type { Blueprint } from "@/types/application";
import type { ApplicationTemplate } from "@/types/applicationTemplate";

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
 * the Rust backend as plain English (`apps/desktop/src-tauri/src/blueprints/*.rs`) -
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
  nats: {
    name: "NATS",
    description:
      "Samodzielny serwer NATS - lekki broker wiadomości do pub/sub, request/reply i kolejek zadań między aplikacjami. Włącz JetStream, żeby mieć trwałość; dane leżą we własnym katalogu roboczym tej aplikacji.",
    fields: {
      natsVersion: { label: "Wersja NATS", helpText: "Tag z Docker Huba, np. 2, 2.10 albo alpine." },
      jetStream: {
        label: "Włącz JetStream (trwałość)",
        helpText:
          "Zapisuje strumienie we własnym katalogu roboczym tej aplikacji, więc przeżywają odtworzenie kontenera. Wyłączone znaczy, że wiadomości istnieją tylko w pamięci.",
      },
      authToken: {
        label: "Token uwierzytelniający",
        helpText:
          "Ustawia --auth. Puste znaczy, że serwer przyjmie każdego klienta - bezpieczne tylko wtedy, gdy ten port pozostaje prywatny (zobacz widoczność w zakładce Porty).",
      },
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
  postgres: {
    // The name stays: "PostgreSQL" is what it is called in Polish too.
    description:
      "Serwer bazy danych PostgreSQL â dane leÅ¼Ä w katalogu roboczym tej aplikacji. Zanim jÄ uruchomisz, ustaw w zakÅadce Årodowisko zmienne POSTGRES_PASSWORD oraz PGDATA=. (sama kropka, czyli katalog tej aplikacji). Bez PGDATA baza powstanie wewnÄtrz kontenera, a ponowne utworzenie kontenera jÄ skasuje.",
    fields: {
      postgresVersion: { label: "Wersja PostgreSQL", helpText: "Tag z Docker Hub, np. 17, 16, 15 albo 17-alpine." },
    },
  },
  mongodb: {
    name: "MongoDB",
    description:
      "Samodzielna baza dokumentowa MongoDB - dane leżą we własnym katalogu roboczym tej aplikacji. Zanim ją uruchomisz, ustaw MONGO_INITDB_ROOT_USERNAME i MONGO_INITDB_ROOT_PASSWORD w zakładce Środowisko: razem tworzą konto administratora i włączają uwierzytelnianie.",
    fields: {
      mongodbVersion: { label: "Wersja MongoDB", helpText: "Tag z Docker Huba, np. 8, 7 albo 6." },
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
      "Webowy interfejs do zarządzania serwerem MySQL/MariaDB. Wskaż bazę, którą ma obsługiwać, a VibeSSH ustawi PMA_HOST/PMA_PORT i przyzna połączenie między nimi; żeby sięgnąć do serwera spoza VibeSSH, zostaw to pole puste i ustaw PMA_HOST samodzielnie w zakładce Środowisko.",
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

/**
 * Polish names for the templates that ship with VibeSSH.
 *
 * Keyed by id rather than by name, because the name is the thing being
 * replaced. The ids are fixed in `apps/desktop/src-tauri/src/storage/builtin_templates.rs`
 * and pinned there by a test that asserts these exact strings - a built-in
 * whose id changed would quietly fall back to its English name rather than
 * breaking, which is why that test exists.
 *
 * A template the user saved themselves is never touched: they named it.
 */
const PL_BUILTIN_TEMPLATE_NAMES: Record<string, string> = {
  "7b1d0001-0000-4000-8000-564249424553": "MariaDB z hasłem roota",
  "7b1d0002-0000-4000-8000-564249424553": "phpMyAdmin do aplikacji MariaDB",
  "7b1d0003-0000-4000-8000-564249424553": "phpMyAdmin do dowolnego serwera",
  "7b1d0004-0000-4000-8000-564249424553": "MongoDB z kontem administratora",
  "7b1d0005-0000-4000-8000-564249424553": "PostgreSQL z hasłem i katalogiem danych",
};

/** The name to show for a template - translated only for a built-in. */
export function translateTemplateName(template: ApplicationTemplate, language: string): string {
  if (!template.isBuiltin || !language.startsWith("pl")) return template.name;
  return PL_BUILTIN_TEMPLATE_NAMES[template.id] ?? template.name;
}
