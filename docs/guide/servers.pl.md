---
id: servers
title: Serwery
section: nodes
route: /servers
order: 10
---

Node to serwer lub VPS dodany do VibeSSH.

![Lista serwerów z ich adresami i stanem](images/servers-list.png)

## Jak dodać Node

1. Otwórz **Serwery**.
2. Kliknij **Dodaj serwer**.
3. Zostaw wybraną zakładkę **Połącz przez SSH**.
4. **Nazwa** — dowolna nazwa dla Ciebie, np. `Serwer produkcyjny`.
5. **Host** — adres IP serwera, np. `203.0.113.10`.
6. **Port** — zostaw `22`, chyba że dostawca podał inny.
7. **Nazwa użytkownika** — konto na serwerze, zwykle `root`.
8. **Uwierzytelnianie** — wybierz **Hasło** albo **Klucz SSH**.
9. Przy haśle wpisz hasło. Przy kluczu wskaż plik klucza na tym komputerze.
10. Kliknij **Testuj połączenie**. Powinno pojawić się **Połączono pomyślnie**.
11. Kliknij **Zapisz serwer**.

## Jak skonfigurować Node

1. Na karcie serwera kliknij ikonę konfiguracji Node'a.
2. W sekcji **Wymagania** zobaczysz Docker, WireGuard i Firewall (ufw).
3. Przy pozycji ze statusem **Brak** kliknij **Zainstaluj automatycznie**.
4. Poczekaj, aż status zmieni się na **Zainstalowane**.
5. Kliknij **Sprawdź ponownie**, jeśli chcesz odświeżyć stan.

## Jak usunąć Node

1. Otwórz **Serwery**.
2. Na karcie serwera kliknij ikonę kosza.
3. Potwierdź.

Usunięcie Node'a w VibeSSH nie kasuje niczego na samym serwerze.

## Jak sprawdzić, czy działa

- Serwer ma status **Online**.
- Na karcie widać CPU, RAM i czas pracy.
- **Terminal** otwiera połączenie z serwerem.

## Najczęstsze problemy

- **Testuj połączenie kończy się błędem** — sprawdź adres IP, port i nazwę użytkownika. Sprawdź, czy serwer jest włączony.
- **Test przechodzi, ale instalacja Dockera się nie udaje** — konto nie ma uprawnień `sudo`. Użyj konta `root` albo nadaj uprawnienia.
- **Nagle pojawia się ostrzeżenie o kluczu hosta** — VibeSSH zapamiętał wcześniejszy klucz serwera. Jeśli sam stawiałeś serwer od nowa, wszystko się zgadza. Jeśli nie, nie łącz się i sprawdź, co się stało.

## Więcej informacji

Hasła i hasła kluczy trafiają do magazynu poświadczeń systemu, nie do pliku konfiguracyjnego. Przy edycji serwera puste pole hasła oznacza „zostaw obecne". Tryb **Zainstaluj Vibe Agenta** instaluje usługę na serwerze zamiast łączyć się po SSH; Node w tym trybie nie pokazuje CPU i RAM i nie ma do niego terminala.
