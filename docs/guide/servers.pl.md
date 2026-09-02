---
id: servers
title: Serwery
section: nodes
route: /servers
order: 1
---

Serwer, nazywany też węzłem, to maszyna, którą VibeSSH obsługuje. Wszystko inne — aplikacje, pliki, terminal, monitoring — dzieje się na którymś z nich, więc dodanie serwera jest pierwszą rzeczą, jaką się tutaj robi.

![Lista serwerów z ich adresami i stanem](images/servers-list.png)

## Dwa tryby połączenia

**Połącz przez SSH** — VibeSSH łączy się z serwerem tak, jak zrobiłby to człowiek: po SSH, Twoimi poświadczeniami. Na serwerze nie instaluje się nic. To domyślny i najlepiej wspierany tryb.

**Zainstaluj Vibe Agenta** — na serwerze stoi mała usługa, z którą aplikacja rozmawia własnym protokołem. Przydaje się tam, gdzie SSH nie jest wygodne. Węzeł w trybie Agent nie raportuje metryk CPU i RAM, tylko stan synchronizacji, i nie ma do niego terminala z tej aplikacji.

## Pola połączenia SSH

| Pole | Znaczenie | Przykład |
| --- | --- | --- |
| Nazwa | Etykieta w aplikacji. Możesz ją zmieniać dowolnie. | `Serwer produkcyjny` |
| Host | Adres IP albo nazwa domenowa. | `203.0.113.10` |
| Port | Port SSH. | `22` |
| Nazwa użytkownika | Konto na serwerze. | `root` |
| Uwierzytelnianie | Hasło albo klucz SSH. | `Klucz SSH` |
| Ścieżka klucza | Plik klucza prywatnego na tym komputerze. | `C:\Users\ty\.ssh\id_ed25519` |

**Hasła i hasła kluczy trafiają do magazynu poświadczeń systemu**, nigdy do zwykłego pliku konfiguracyjnego. Przy edycji istniejącego serwera puste pole hasła oznacza „zostaw obecne", a nie „usuń".

## Testuj połączenie

Otwiera prawdziwe połączenie SSH i natychmiast je zamyka. **Nic nie zapisuje** — możesz testować, poprawiać i testować ponownie, zanim cokolwiek trafi na listę.

Warto go użyć zawsze przy pierwszym dodaniu: błąd zobaczysz od razu i z konkretnym powodem, zamiast dowiadywać się o nim przy pierwszej operacji na plikach.

## Klucz hosta

Przy pierwszym połączeniu VibeSSH zapamiętuje klucz publiczny serwera. Jeśli przy kolejnym połączeniu klucz się nie zgadza, połączenie jest **przerywane z błędem**, a nie po cichu akceptowane.

Zwykle znaczy to, że serwer został postawiony od nowa albo przeinstalowany. Ale to jest dokładnie ten sam sygnał, który pojawia się przy podszywaniu się pod serwer, więc VibeSSH nie zgaduje, o który przypadek chodzi — decyzja należy do Ciebie.

## Uprawnienia

Wiele operacji na węźle wymaga `sudo`: instalacja Dockera, reguły firewalla, zakładanie kont dedykowanych dla aplikacji, WireGuard. Konto bez `sudo` obsłuży część funkcji, ale nie wszystkie, a komunikat błędu powie wprost, że sudo odmówiło.

## Częste pomyłki

- **Test przechodzi, a operacje padają** — najczęściej brak `sudo` dla tego konta.
- **Nagle „niezgodność klucza hosta"** — potraktuj to poważnie, zanim usuniesz wpis. Jeśli to Ty przeinstalowałeś maszynę, wszystko się zgadza; jeśli nie, warto sprawdzić dlaczego.
- **Zmieniłem hasło i przestało działać** — hasło trzymane jest w magazynie systemowym per serwer. Po zmianie na serwerze trzeba je zaktualizować także tutaj.
