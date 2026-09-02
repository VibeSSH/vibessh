---
id: settings
title: Ustawienia
section: getting-started
route: /settings
order: 90
---

Ustawienia zbierają rzeczy, które dotyczą całej aplikacji, a nie pojedynczego węzła: język, dostęp do zewnętrznych usług i konfigurację asystenta.

## Preferencje

**Język** przełącza interfejs między polskim a angielskim. Ustawienie jest osobiste i **tylko na tym urządzeniu** — nie synchronizuje się nigdzie.

Poradnik idzie za tym ustawieniem. Rozdział, którego nie ma jeszcze w Twoim języku, pokaże się w tym, w którym istnieje, i wyraźnie to napisze.

## Cel backupów

Zewnętrzny magazyn zgodny z S3, do którego kopiowane są backupy aplikacji. Wypełniasz **Endpoint**, **Region**, **Bucket** i parę kluczy.

**Adresowanie path-style** trzeba zaznaczyć dla większości instalacji MinIO — to nie jest opcja kosmetyczna, bez niej połączenie po prostu nie zadziała.

Przy edycji **puste pole Secret Access Key oznacza „zostaw obecny"**, a nie „usuń". Klucz trafia do magazynu poświadczeń systemu.

**Testuj połączenie** sprawdza dostęp, zanim pierwszy backup spróbuje się wysłać. Warto — inaczej dowiesz się o błędnej konfiguracji w momencie, w którym backup miał już istnieć.

> Backup leżący tylko na tym samym węźle co aplikacja chroni przed pomyłką, ale nie przed utratą węzła.

## Prywatne rejestry Docker

Dane logowania do rejestrów obrazów. **Obrazy publiczne działają bez logowania** — rejestr dodaje się tylko wtedy, gdy potrzebny jest obraz prywatny.

Jeden wpis na rejestr, używany przez każdą aplikację, której obraz stamtąd pochodzi. Adresem jest host: `docker.io`, `ghcr.io` albo adres własnego rejestru. Hasło albo token dostępu trafia do magazynu poświadczeń.

## Sufiks DNS

Końcówka nazw w prywatnym DNS Vibe Network — domyślnie `.vibe`, więc węzeł nazywa się `serwer.vibe`. Zmiana dotyczy wszystkich aliasów, więc po niej warto zsynchronizować Vibe Network.

## Vibe AI

Włączenie asystenta, wybór dostawcy, adres bazowy, model, klucz API i test połączenia. Tutaj też widać zużycie dziennego limitu modelu współdzielonego. Szczegóły opisuje rozdział o Vibe AI.

**Klucz API trafia do magazynu poświadczeń systemu i nie wraca do interfejsu po zapisaniu.** Puste pole przy edycji znaczy „zostaw obecny".

## O aplikacji

Wersja i sprawdzenie, czy backend Rust odpowiada. Jeśli utknie na „Oczekiwanie na backend", interfejs działa, ale nic pod nim — żadna operacja się nie powiedzie.

## Częste pomyłki

- **Wyczyściłem pole klucza, żeby go usunąć** — puste pole zachowuje obecny klucz. Do usunięcia służy osobna akcja przy danym wpisie.
- **Backupy nie trafiają do S3** — użyj Testuj połączenie i sprawdź path-style; przy MinIO to najczęstsza przyczyna.
- **Zmieniłem język i poradnik jest po angielsku** — ten rozdział nie ma jeszcze wersji w wybranym języku. Strona to napisze wprost.
