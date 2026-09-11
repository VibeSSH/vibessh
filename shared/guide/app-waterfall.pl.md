---
id: app-waterfall
title: Waterfall
section: blueprints
route: /applications
order: 130
---

Waterfall to **proxy** oparte na BungeeCord — jeden adres dla graczy, za nim kilka serwerów Minecraft.

Robi to samo co **Velocity**. Wybierz Waterfall, kiedy masz wtyczki napisane pod BungeeCord, których nie chcesz zamieniać. Do nowej sieci serwerów lepszym wyborem jest Velocity: jest szybsze i aktywnie rozwijane.

## Krok po kroku

1. Postaw najpierw serwery, które mają stać za proxy (zwykle **Paper**), bez publicznego portu.
2. **Aplikacje → Nowa aplikacja**, ta sama lokalizacja co serwery.
3. Wybierz **Waterfall**, nadaj nazwę, np. `proxy`.
4. **Wersja Waterfall** i **Wersja Javy** — zostaw domyślne.
5. Utwórz i uruchom raz, żeby powstał plik `config.yml`.

## Jak podłączyć serwery

1. Zakładka **Porty** → karta **Połączenia** → połącz proxy z każdym serwerem.
2. **Pliki** → `config.yml`, sekcja `servers`, adresy to **nazwy aplikacji małymi literami** — przykład pod listą.
3. W `config.yml` ustaw `ip_forward: true`.
4. W każdym serwerze za proxy: `server.properties` → `online-mode=false`, a w `spigot.yml` → `settings.bungeecord: true`.
5. Uruchom ponownie proxy i serwery.

Sekcja `servers` wygląda tak:

```
servers:
  lobby:
    address: lobby:25565
    restricted: false
```

## Porty

Publiczny jest tylko port proxy. Serwery za nim zostaw bez publikacji albo na **Vibe Network**.

## Częste problemy

**Gracz wchodzi i wylatuje.** Zwykle brak `ip_forward` po stronie proxy albo `bungeecord: true` po stronie serwera.

**Serwer nieosiągalny z proxy.** Sprawdź kartę **Połączenia** i adres w `config.yml`.
