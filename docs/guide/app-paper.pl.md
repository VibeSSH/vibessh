---
id: app-paper
title: Paper
section: blueprints
route: /applications
order: 100
---

Paper to serwer Minecraft. Wersja rozwojowa Spigota — szybsza, z większą liczbą ustawień i wsparciem dla wtyczek Bukkit/Spigot/Paper.

VibeSSH sam pobiera plik serwera i pilnuje wersji. Nie musisz nic ściągać ręcznie ani wgrywać przez FTP.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**.
2. Wybierz lokalizację: **ten komputer** albo Node. Serwer dla znajomych zwykle stawia się na Node, żeby działał, gdy Twój komputer jest wyłączony.
3. Nadaj nazwę, np. `survival`. Ta nazwa staje się też adresem, pod którym widzą tę aplikację inne aplikacje na tym samym Node.
4. Wybierz **Paper** z listy rodzajów.
5. **Wersja Minecraft** — wybierz z listy, np. `1.21.11`. Musi zgadzać się z wersją, z której łączą się gracze.
6. **Akceptuję EULA** — zaznacz. Bez tego serwer wystartuje raz i zgaśnie, wypisując prośbę o akceptację licencji Mojanga.
7. **Wersja Javy** — zostaw `21`. Nowsze wersje Minecrafta wymagają nowszej Javy; jeśli serwer zgłosi błąd o `class file version`, podnieś tę wartość.
8. **Argumenty JVM** — zostaw puste albo wklej gotowy zestaw z [flags.sh](https://flags.sh). Tutaj ustawia się pamięć, np. `-Xmx4G`.
9. Utwórz aplikację i kliknij **Uruchom**.

Port **25565** zostaje dodany i opublikowany automatycznie, bo bez niego nikt się nie połączy. Zmienisz go w zakładce **Porty**.

## Pierwsze uruchomienie

W zakładce **Konsola** zobaczysz start serwera. `Done (12.345s)! For help, type "help"` oznacza, że działa i można wchodzić.

Konsola jest interaktywna — możesz w nią pisać polecenia serwera, np. `op TwojNick` albo `stop`. Kolory z czatu i logów są pokazywane tak, jak wysyła je serwer.

## Wtyczki i konfiguracja

- **Pliki → wgraj** — wrzuć plik `.jar` wtyczki do katalogu `plugins`, potem **Uruchom ponownie**.
- **Pliki → Szybkie pliki** — `server.properties`, `bukkit.yml`, `spigot.yml`, `paper-global.yml` i `paper-world-defaults.yml` są jedno kliknięcie od Ciebie, bez szukania po drzewie katalogów.
- Baza dla wtyczek (LuckPerms, sklepy) — patrz **MariaDB** albo zakładka **Bazy danych**.

## Zmiana wersji

Otwórz **Ustawienia → Konfiguracja → Edytuj** i wybierz inną **Wersję Minecraft**. Po zapisaniu VibeSSH pobierze odpowiedni plik serwera i odtworzy kontener.

Przed zmianą wersji zrób kopię w zakładce **Backupy**. Świat zapisany w nowszej wersji zwykle nie otworzy się już w starszej.

## Częste problemy

**Serwer gaśnie zaraz po starcie, w logach coś o EULA.** Nie zaznaczyłeś akceptacji licencji. Ustawienia → Konfiguracja → Edytuj → zaznacz **Akceptuję EULA**.

**`Unsupported class file major version` albo `UnsupportedClassVersionError`.** Wtyczka albo sam serwer wymaga nowszej Javy. Podnieś **Wersję Javy** w konfiguracji.

**Gracze nie mogą wejść.** Sprawdź zakładkę **Porty**: czy port `25565` jest opublikowany i ustawiony jako **Publiczny**, a potem kliknij **Zsynchronizuj firewall**. Podaj graczom adres Node'a, nie adres swojego komputera.

**Serwer zjada całą pamięć Node'a.** Ustaw limit w **Ustawieniach → Limity zasobów** i dopasuj `-Xmx` w argumentach JVM do tego limitu.
