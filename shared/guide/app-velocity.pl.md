---
id: app-velocity
title: Velocity
section: blueprints
route: /applications
order: 120
---

Velocity to **proxy** — jeden adres, pod który łączą się gracze, i za nim kilka serwerów Minecraft, między którymi można się przenosić bez rozłączania.

Sam nie jest serwerem. Nie ma świata, nie ma wtyczek Bukkit, nie da się w nim grać. Kieruje ruchem do serwerów, które stawiasz osobno.

## Kiedy tego potrzebujesz

Kiedy masz więcej niż jeden serwer i chcesz, żeby gracze wpisywali jeden adres: lobby, survival, minigry. Przy jednym serwerze Velocity nie jest do niczego potrzebne.

## Krok po kroku

1. Najpierw postaw serwery, które mają być za proxy — zwykle **Paper**. Nie publikuj im portu `25565` na zewnątrz; gracze mają wchodzić przez proxy.
2. **Aplikacje → Nowa aplikacja**, wybierz **tę samą lokalizację** co serwery.
3. Wybierz **Velocity**, nadaj nazwę, np. `proxy`.
4. **Wersja Velocity** — wybierz z listy.
5. **Wersja Javy** — zostaw `21`.
6. Utwórz aplikację i kliknij **Uruchom** raz, żeby powstały pliki konfiguracyjne.

## Jak podłączyć serwery do proxy

1. Otwórz aplikację proxy → zakładka **Porty** → karta **Połączenia**. Połącz proxy z każdym serwerem, który ma być za nim. Bez tego proxy ich nie zobaczy.
2. **Pliki → Szybkie pliki → velocity.toml**. W sekcji `[servers]` wpisz serwery, używając **nazw aplikacji małymi literami** — przykład pod listą.
3. Skopiuj zawartość pliku `forwarding.secret` (też w Szybkich plikach).
4. W każdym serwerze za proxy: `server.properties` → `online-mode=false`, a w `config/paper-global.yml` włącz `velocity` i wklej tam ten sam sekret.
5. Uruchom ponownie proxy i serwery.

Sekcja `[servers]` wygląda tak:

```
[servers]
lobby = "lobby:25565"
survival = "survival:25565"
try = ["lobby"]
```

Krok 4 jest obowiązkowy. Serwer za proxy z `online-mode=true` odrzuci graczy, a bez sekretu można się do niego podłączyć z pominięciem proxy i wejść na dowolny nick.

## Porty

Port `25565` proxy ma być **Publiczny** — to jego adres podają gracze. Porty serwerów za proxy zostaw nieopublikowane albo ustaw na **Vibe Network**.

## Częste problemy

**Proxy nie widzi serwera.** Brak połączenia w karcie **Połączenia**, albo w `velocity.toml` wpisany zły adres. Adres to nazwa aplikacji małymi literami, spacje jako myślniki.

**Gracz wchodzi i od razu wylatuje.** Najczęściej `online-mode` albo niezgodny sekret przekazywania. Sprawdź krok 4.
