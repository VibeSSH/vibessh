---
id: intellij
title: Wtyczka do IntelliJ
section: getting-started
route: /settings
order: 146
---

Wtyczka wysyła zbudowany plik wprost z IDE do aplikacji w VibeSSH i restartuje ją.
Zamiast lokalnego serwera deweloperskiego pracujesz na tym samym serwerze, na którym
wtyczka potem żyje.

Działa też bez niej — przez zadanie Gradle opisane na stronie **Claude i inni asystenci**.
Różnica jest jedna i ważna: **wtyczka trzyma token w magazynie haseł IDE**, a zadanie
Gradle w pliku, który zwykle leży w repozytorium.

## Zanim zaczniesz

1. VibeSSH w wersji **0.1.0-beta.17** lub nowszej, **uruchomiony** (schowany do zasobnika
   też się liczy).
2. **Ustawienia → Claude i inni asystenci** — włączone, razem z **Zezwól na zmiany**,
   bo wtyczka zapisuje plik na serwerze.

## Instalacja

Wtyczki nie ma jeszcze w Marketplace, więc instaluje się ją z pliku.

1. Zbuduj paczkę:

   ```bash
   cd apps/intellij
   ./gradlew buildPlugin
   ```

   Powstanie `build/distributions/vibessh-intellij-0.1.0.zip`.

2. W IntelliJ: **Settings → Plugins → ⚙ → Install Plugin from Disk...**, wskaż ten plik.
3. Zrestartuj IDE.

Jeśli wtyczka nie chce się wczytać, zbuduj ją pod swoją wersję IDE:

```bash
./gradlew buildPlugin -PvibesshIdePath="C:/Program Files/JetBrains/IntelliJ IDEA 2026.1"
```

Kompilator sprawdzi wtedy, czy wszystkie używane API istnieją w tym wydaniu — zamiast
pozwolić wtyczce zainstalować się i wywalić przy pierwszym użyciu.

## Konfiguracja

**Settings → Tools → VibeSSH**:

| pole | co wpisać |
|---|---|
| Port | `7422`, chyba że zmieniałeś go w VibeSSH |
| Token | z **Ustawienia → Claude i inni asystenci → Token → Pokaż** |
| Katalog docelowy | `plugins` dla wtyczek Minecrafta |
| Zrestartuj po wgraniu | zostaw włączone przy serwerze testowym |

Kliknij **Testuj połączenie**. Poprawna odpowiedź brzmi „Połączono z vibessh 0.1.0-beta.17"
i oznacza trzy rzeczy naraz: VibeSSH działa, endpoint jest włączony, a token się zgadza.

Token trafia do magazynu haseł IDE, opartego o pęk kluczy systemu. **Nie ląduje w `.idea/`
ani w żadnym pliku projektu**, więc nie ma jak trafić do repozytorium.

## Wdrażanie

**Build → Deploy to VibeSSH.**

1. Wybierasz plik — okno otwiera się w `build/libs`, jeśli taki katalog istnieje.
2. Wybierasz aplikację z listy pobranej z VibeSSH.
3. Pasek stanu pokazuje postęp, a wynik przychodzi jako powiadomienie.

Wybór aplikacji jest przy każdym wdrożeniu, nie w ustawieniach — bo pomyłka oznacza
wtyczkę lądującą na serwerze, na którym nie miała się znaleźć, i restart tego serwera.
Ostatni wybór jest podpowiadany.

## Na co uważać

- **Restart dotyczy prawdziwego serwera.** Przy produkcyjnym wyłącz **Zrestartuj po
  wgraniu** i zrestartuj świadomie, kiedy nikt nie gra.
- **Ścieżka jest względna wobec katalogu roboczego aplikacji.** Próba wyjścia poza niego
  jest odrzucana po stronie VibeSSH.
- **Po wygenerowaniu nowego tokenu** trzeba wkleić go ponownie w ustawieniach wtyczki —
  stary przestaje działać natychmiast.

## Jak sprawdzić, czy działa

- **Testuj połączenie** zwraca wersję VibeSSH.
- Po wdrożeniu **Pliki** w VibeSSH pokazują plik w katalogu docelowym z dzisiejszą datą.
- **Logi** aplikacji pokazują start z wgraną wtyczką.

## Najczęstsze problemy

**„Najpierw wklej token"** — pole tokenu w ustawieniach wtyczki jest puste.

**„Nie udało się połączyć z VibeSSH"** — aplikacja nie działa albo endpoint jest
wyłączony. Schowana do zasobnika działa; zamknięta przez **Zakończ VibeSSH** nie.

**„VibeSSH odrzucił token"** — token się nie zgadza, najczęściej po wygenerowaniu nowego.
Skopiuj go ponownie.

**„VibeSSH ma wyłączone Zezwól na zmiany"** — wtyczka zapisuje plik na serwerze, więc
potrzebuje tej zgody. Włącz ją w Ustawieniach VibeSSH.

**Wtyczka nie pojawia się po instalacji** — zrestartuj IDE; katalog wtyczek czytany jest
przy starcie. Jeśli to nie pomoże, zbuduj ją z `-PvibesshIdePath` wskazującym Twoją
instalację.
