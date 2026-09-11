---
id: app-generic
title: Aplikacja ogólna
section: blueprints
route: /applications
order: 160
---

Uruchamia dowolne polecenie na Node albo na tym komputerze — bez Dockera, bezpośrednio jako proces.

Wybierz to, kiedy masz gotowy program albo skrypt i chcesz tylko, żeby coś go pilnowało, pokazywało logi i pozwalało restartować jednym kliknięciem.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**, wybierz lokalizację i nazwę.
2. Wybierz **Aplikacja ogólna** z listy rodzajów.
3. **Polecenie** — pełna ścieżka do pliku wykonywalnego, np. `/usr/bin/python3`.
4. **Argumenty** — po jednym na wiersz, w kolejności, np. `main.py`.
5. **Polecenie zatrzymania** — opcjonalne. Jeśli program potrzebuje uporządkowanego wyłączenia (jak `stop` w serwerze Minecraft), wpisz je tutaj. Bez tego VibeSSH wysyła zwykły sygnał zakończenia.
6. Utwórz aplikację i kliknij **Uruchom**.

## Katalog roboczy

Polecenie wykonuje się w katalogu roboczym aplikacji. Ścieżki względne w argumentach odnoszą się właśnie do niego.

## Czym to się różni od kontenera

Proces działa bezpośrednio na maszynie: widzi jej system plików i jej zainstalowane pakiety. Nie ma izolacji, którą daje kontener, więc nie ma tu też prywatnej sieci ani karty **Połączenia** — do innych usług łączy się zwykłym adresem i portem.

## Częste problemy

**`No such file or directory`.** Pole **Polecenie** wymaga pełnej ścieżki. Sprawdź w terminalu poleceniem `which nazwa`.

**Program nie chce się zatrzymać.** Wypełnij **Polecenie zatrzymania** albo użyj **Wymuś zakończenie**.
