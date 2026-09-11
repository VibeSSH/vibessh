---
id: app-python-bot
title: Bot Python
section: blueprints
route: /applications
order: 260
---

Uruchamia Twój własny program w Pythonie — bota, skrypt, małe API. VibeSSH pokazuje jego logi, restartuje go i pilnuje, żeby działał.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**, wybierz lokalizację i nazwę, np. `bot`.
2. Wybierz **Bot Python** z listy rodzajów.
3. **Plik startowy** — ścieżka względem katalogu roboczego, np. `bot.py` albo `src/main.py`.
4. **Wersja Pythona** — zostaw `3.13`, chyba że Twój kod wymaga starszej.
5. **Argumenty programu** — zwykle puste.
6. Utwórz aplikację, ale **jeszcze jej nie uruchamiaj**.
7. Zakładka **Pliki** — wgraj kod razem z `requirements.txt`.
8. Zakładka **Ustawienia → Środowisko** — dodaj token albo klucz API i zaznacz **Sekret**.
9. Kliknij **Uruchom**.

## Zależności z requirements.txt

Ta aplikacja uruchamia `python TWÓJ-PLIK` i nie wykonuje `pip install` sama z siebie. Zainstaluj biblioteki w zakładce **Akcje** albo w terminalu, w katalogu roboczym aplikacji.

## Aktualizacja kodu

Wgraj nowe pliki w zakładce **Pliki** i kliknij **Uruchom ponownie**.

## Częste problemy

**`ModuleNotFoundError`.** Nie zainstalowano zależności — patrz sekcja wyżej.

**Program kończy się natychmiast bez błędu.** Skrypt, który po prostu się wykonał, kończy się poprawnie. Bot ma działać w pętli — sprawdź, czy nie zapomniałeś wywołać `run()` albo `asyncio.run(...)`.

**Polskie znaki w logach wyglądają dziwnie.** Ustaw zmienną `PYTHONIOENCODING` na `utf-8`.
