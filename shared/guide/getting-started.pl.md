---
id: getting-started
title: Pierwsze kroki
section: getting-started
route: /
order: 1
---

Od pustej aplikacji do działającego serwera w pięciu krokach.

## 1. Dodaj serwer

1. Otwórz **Serwery**.
2. Kliknij **Dodaj serwer**.
3. Wypełnij pola i kliknij **Testuj połączenie**.
4. Kliknij **Zapisz serwer**.

Szczegóły: rozdział **Serwery**.

## 2. Skonfiguruj Node

1. Na karcie serwera kliknij ikonę konfiguracji.
2. Przy brakujących pozycjach kliknij **Zainstaluj automatycznie**.
3. Poczekaj, aż wszystkie wymagania mają status **Zainstalowane**.

## 3. Utwórz aplikację

1. Otwórz **Aplikacje**.
2. Kliknij **Utwórz aplikację**.
3. Przejdź pięć kroków kreatora i zatwierdź.
4. Na stronie aplikacji kliknij **Uruchom**.

## 4. Otwórz port

1. Wejdź w aplikację → zakładka **Porty**.
2. Sprawdź, czy port ma właściwy **Dostęp sieciowy**.
3. Kliknij **Zsynchronizuj firewall**.

## 5. Włącz backupy

1. Wejdź w aplikację → zakładka **Backupy**.
2. Zaznacz **Twórz backupy automatycznie**.
3. Ustaw odstęp i ile kopii zachować.
4. Kliknij zapis.

## Jak sprawdzić, czy działa

- Node ma status **Online**.
- Aplikacja ma status **Działa**.
- W zakładce **Przegląd** widać konsolę z logami.
- Gracze łączą się na adres IP serwera i port z zakładki **Porty**.

## Najczęstsze problemy

- **Nie mogę utworzyć aplikacji** — na Node brakuje Dockera. Wróć do kroku 2.
- **Port nie odpowiada z internetu** — sprawdź **Dostęp sieciowy** i kliknij **Zsynchronizuj firewall**. Sprawdź też firewall w panelu dostawcy VPS.
- **Zmieniłem plik konfiguracyjny i nic się nie zmieniło** — zrestartuj aplikację przyciskiem **Uruchom ponownie**.
