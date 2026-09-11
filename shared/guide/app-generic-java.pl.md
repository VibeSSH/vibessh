---
id: app-generic-java
title: Aplikacja Java
section: blueprints
route: /applications
order: 140
---

Uruchamia dowolny plik `.jar`. Do wszystkiego, co jest napisane w Javie, a nie jest Paperem, Purpurem, Velocity ani Waterfallem — bota, narzędzia, serwera Minecraft w wersji, której VibeSSH nie pobiera samodzielnie.

Różnica wobec **Papera**: tutaj plik `.jar` wgrywasz sam i sam pilnujesz jego wersji. VibeSSH niczego nie pobiera.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**, wybierz lokalizację i nazwę.
2. Wybierz **Aplikacja Java** z listy rodzajów.
3. **Wersja Javy** — `21` pasuje do większości. Jeśli program zgłosi błąd o `class file version`, podnieś tę wartość.
4. **Plik jar** — ścieżka względem katalogu roboczego, np. `server.jar`.
5. **Argumenty JVM** — pamięć i flagi, np. `-Xmx2G`. Możesz wkleić całą linię `java -Xmx2G -jar server.jar nogui` — VibeSSH rozłoży ją na właściwe pola.
6. **Argumenty programu** — to, co idzie po nazwie jara, np. `nogui`.
7. Utwórz aplikację, wgraj plik `.jar` w zakładce **Pliki**, kliknij **Uruchom**.

## Porty

Żaden port nie jest dodawany automatycznie, bo VibeSSH nie wie, na czym słucha Twój program. Dodaj go sam w zakładce **Porty** i kliknij **Zsynchronizuj firewall**.

## Częste problemy

**`Unable to access jarfile`.** Zła ścieżka w polu **Plik jar** albo plik nie został wgrany. Sprawdź zakładkę **Pliki** — nazwa musi się zgadzać co do znaku, z rozszerzeniem włącznie.

**`UnsupportedClassVersionError`.** Program wymaga nowszej Javy. Podnieś **Wersję Javy**.

**Program działa, ale nikt się nie łączy.** Brakuje opublikowanego portu — patrz wyżej.
