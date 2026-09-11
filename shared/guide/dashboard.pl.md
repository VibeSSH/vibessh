---
id: dashboard
title: Panel
section: getting-started
route: /
order: 5
---

Panel pokazuje stan wszystkich serwerów i aplikacji w jednym miejscu.

![Panel: stan zbiorczy, kafelki węzłów i alerty](images/dashboard.png)

## Co jest na Panelu

1. **Nagłówek** — zbiorczy stan: **Wszystko w porządku** albo **Wymaga uwagi**.
2. **Node'y** — kafelek na serwer z CPU, RAM-em i czasem pracy.
3. **Alerty** — lista problemów: serwer offline, aplikacja z błędem.
4. **Zadania** — rzeczy do wykonania jednym kliknięciem, np. synchronizacja Node'a.

## Jak sprawdzić jeden serwer

1. Kliknij kafelek serwera.
2. Pod spodem pojawią się zakładki **Aplikacje**, **Terminal** i **Aktywność**.
3. Kliknij **Wszystkie Node'y**, żeby wrócić do widoku zbiorczego.

## Jak sprawdzić, czy działa

- Kafelki pokazują procenty CPU i RAM.
- Serwery mają status **Online**.
- Sekcja **Alerty** jest pusta.

## Najczęstsze problemy

- **Serwer pokazuje offline, choć działa** — VibeSSH nie mógł się połączyć. Sprawdź serwer w module **Serwery**.
- **Kafelek pokazuje „Zbieranie danych…"** — brak jeszcze drugiego odczytu. Poczekaj chwilę.
- **Metryki stoją w miejscu** — okno było schowane. Odświeżanie rusza po powrocie.
