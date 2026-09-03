---
id: port-forwarding
title: Tunele SSH
section: nodes
route: /port-forwarding
order: 90
---

Tunel pozwala połączyć się z usługą na serwerze, która nie jest dostępna z internetu — na przykład z panelem bazy danych.

## Jak utworzyć tunel

1. Otwórz **Przekierowanie portów** i wybierz serwer.
2. Kliknij **Nowy tunel**.
3. **Typ tunelu** — wybierz **Lokalny**.
4. **Adres nasłuchu** — wpisz `127.0.0.1`.
5. **Port nasłuchu** — numer na Twoim komputerze, np. `3306`. Wpisz `0`, żeby system wybrał wolny.
6. **Adres docelowy** — adres widziany z serwera, zwykle `127.0.0.1`.
7. **Port docelowy** — port usługi na serwerze, np. `3306`.
8. Kliknij **Uruchom**.

## Przykład

Dostęp do bazy MariaDB na serwerze:

- Typ tunelu: **Lokalny**
- Adres nasłuchu: `127.0.0.1`
- Port nasłuchu: `3306`
- Adres docelowy: `127.0.0.1`
- Port docelowy: `3306`

Po uruchomieniu w kliencie bazy łączysz się z `127.0.0.1:3306`.

## Jak zatrzymać tunel

1. Znajdź tunel na liście.
2. Kliknij ikonę zatrzymania.

## Jak sprawdzić, czy działa

- Tunel jest na liście aktywnych.
- Program na Twoim komputerze łączy się z podanym portem lokalnym.

## Najczęstsze problemy

- **Port nasłuchu jest zajęty** — coś na Twoim komputerze już go używa. Wpisz `0`.
- **Tunel zniknął po restarcie aplikacji** — tunele nie są zapisywane. Utwórz go ponownie.
- **Nie łączy się z celem** — **Adres docelowy** jest rozwiązywany po stronie serwera. Wpisz adres, który widzi serwer, zwykle `127.0.0.1`.

## Więcej informacji

Tunele działają tylko wtedy, gdy VibeSSH jest otwarte. Do stałego dostępu użyj portu aplikacji z odpowiednim **Dostępem sieciowym**. Typ **Zdalny** działa odwrotnie: port na serwerze prowadzi do Twojego komputera. **Dynamiczny (SOCKS5)** tworzy proxy, przez które ruch wychodzi z serwera.
