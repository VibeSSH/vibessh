---
id: app-nodejs-bot
title: Bot Node.js
section: blueprints
route: /applications
order: 250
---

Uruchamia Twój własny program napisany w Node.js — bota Discorda, skrypt, małe API. VibeSSH pilnuje, żeby działał, pokazuje jego logi i restartuje go, kiedy każesz.

VibeSSH nie pisze kodu za Ciebie. Ta aplikacja bierze pliki, które wgrasz, i uruchamia wskazany plik startowy.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**, wybierz lokalizację i nazwę, np. `bot`.
2. Wybierz **Bot Node.js** z listy rodzajów.
3. **Plik startowy** — ścieżka względem katalogu roboczego, np. `index.js` albo `src/bot.js`.
4. **Wersja Node.js** — zostaw `22`, chyba że Twój kod wymaga innej.
5. **Argumenty programu** — zwykle puste.
6. Utwórz aplikację, ale **jeszcze jej nie uruchamiaj**.
7. Zakładka **Pliki** — wgraj kod bota razem z `package.json`.
8. Zakładka **Ustawienia → Środowisko** — dodaj token bota. Zaznacz przy nim **Sekret**: wartość trafi wtedy do magazynu haseł systemu, a nie do bazy VibeSSH, i nie będzie widoczna później na ekranie.
9. Kliknij **Uruchom**.

## Zależności z package.json

Ta aplikacja uruchamia `node TWÓJ-PLIK`. Nie wykonuje `npm install` za Ciebie.

Najprostsze wyjście: zainstaluj zależności u siebie i wgraj katalog `node_modules` razem z kodem. Alternatywnie użyj zakładki **Akcje** albo terminala Node'a, żeby wykonać `npm install` w katalogu roboczym aplikacji.

## Aktualizacja kodu

1. Wgraj nowe pliki w zakładce **Pliki**.
2. Kliknij **Uruchom ponownie**.

## Częste problemy

**`Cannot find module`.** Brakuje `node_modules` — patrz sekcja wyżej.

**Bot startuje i od razu gaśnie.** Zajrzyj w **Logi**. Najczęściej brakuje tokenu w zmiennych środowiskowych albo plik startowy ma inną ścieżkę niż podana.

**Token widoczny w logach.** Nie wypisuj go w kodzie. Zmienna oznaczona jako **Sekret** nie pojawia się w interfejsie VibeSSH, ale nic nie powstrzyma Twojego własnego `console.log`.
