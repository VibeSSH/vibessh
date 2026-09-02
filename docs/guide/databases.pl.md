---
id: databases
title: Bazy danych
section: applications
route: /database-hosts
order: 45
---

VibeSSH tworzy bazy MySQL/MariaDB dla aplikacji i pilnuje ich danych dostępowych. Nie jest to menedżer baz — nie ma tu przeglądania tabel ani zapytań; do tego jest phpMyAdmin, do którego można stąd przejść.

## Dwa poziomy

**Host bazy danych** to silnik: serwer MySQL albo MariaDB, na którym powstają bazy. Rejestruje się go raz, w menu bocznym pod **Bazy danych**.

**Baza aplikacji** to konkretna baza założona dla jednej aplikacji na wybranym hoście. Tworzy się ją w aplikacji, na zakładce Bazy danych.

Bez zarejestrowanego hosta zakładka w aplikacji nie ma czego zaoferować i powie o tym wprost.

## Rejestrowanie hosta

Podajesz adres, port i konto administracyjne silnika. Konto musi mieć prawo zakładania baz i użytkowników — VibeSSH używa go wyłącznie do tego.

Jeśli na węźle nie ma jeszcze żadnego silnika, dostępny jest przycisk **Zainstaluj MariaDB**. To świadoma decyzja operatora, a nie coś, co dzieje się samo przy tworzeniu aplikacji: instalacja serwera bazy zmienia węzeł trwale.

## Tworzenie bazy dla aplikacji

Wybierasz host, opcjonalnie wpisujesz przeznaczenie (np. `luckperms`) i to wszystko. **Nazwa bazy, login i hasło są generowane automatycznie** — nie wymyślasz ich i nie musisz nigdzie zapisywać.

Osobne konto na bazę jest tu celem, nie ozdobą: aplikacja dostaje dostęp do swojej bazy i tylko do niej.

## Dane dostępowe

Przycisk oka pokazuje host, nazwę bazy, użytkownika i hasło — do wklejenia w konfigurację wtyczki czy aplikacji.

**Hasło nie jest trzymane w konfiguracji aplikacji.** Pokazywane jest na żądanie. Jeśli je zgubisz, nie odzyskujesz go — generujesz nowe przyciskiem resetu, co od razu zmienia hasło w silniku.

> Po zresetowaniu hasła trzeba zaktualizować konfigurację aplikacji i ją zrestartować. Nic nie zrobi tego za Ciebie — stare hasło przestaje działać w tej samej chwili.

## phpMyAdmin

Jeśli host ma skonfigurowany phpMyAdmin, przycisk otwiera go dla tej bazy. To tylko przejście do zewnętrznego narzędzia; VibeSSH nie pośredniczy w zapytaniach.

## Usuwanie

Usunięcie bazy kasuje ją w silniku razem z danymi. Nie da się tego cofnąć i nie ma tu backupu — backupy aplikacji obejmują katalog roboczy, nie zawartość bazy.

## Częste pomyłki

- **„Nie zarejestrowano żadnego hosta"** — najpierw dodaj host w menu bocznym, dopiero potem twórz bazę w aplikacji.
- **Aplikacja nie łączy się z bazą** — sprawdź, czy widzi host. Baza na innym węźle wymaga połączenia przez Vibe Network albo wystawionego portu; sam fakt, że oba węzły są Twoje, nie daje im łączności.
- **Zresetowałem hasło i serwer padł** — konfiguracja aplikacji nadal ma stare. Zaktualizuj i zrestartuj.
