---
id: app-mongodb
title: MongoDB
section: blueprints
route: /applications
order: 215
---

MongoDB to baza dokumentowa. Zamiast tabel i kolumn trzyma dokumenty przypominające JSON, więc nie trzeba z góry ustalać, jakie pola będą w środku.

Używają jej boty, panele i aplikacje pisane w Node.js — wszędzie tam, gdzie kształt danych zmienia się w trakcie życia projektu. Jeśli Twoja wtyczka albo aplikacja prosi o „MySQL" albo „MariaDB", to nie jest to — zajrzyj do poradnika o **MariaDB**.

## Zanim zaczniesz

Przygotuj login i hasło administratora. W MongoDB te dwie zmienne robią więcej, niż wygląda: obraz tworzy z nich konto administratora **i dopiero ich ustawienie włącza uwierzytelnianie**. Baza uruchomiona bez nich przyjmie każdego, kto do niej dojdzie, bez pytania o hasło.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**.
2. Na liście **Szablony** wybierz **MongoDB z kontem administratora**. Szablon wpisuje obie zmienne, o których mowa wyżej.
3. Wybierz lokalizację (ten komputer albo Node) i nadaj nazwę, np. `mongo`.
4. **Wersja MongoDB** — zostaw `8`, chyba że Twoja aplikacja wymaga starszej.
5. W kroku ze **Środowiskiem** uzupełnij wartości:
   - **MONGO_INITDB_ROOT_USERNAME** — login administratora, np. `root`.
   - **MONGO_INITDB_ROOT_PASSWORD** — jego hasło. Zostaw zaznaczone **Sekret**.
6. Utwórz aplikację i kliknij **Uruchom**.

Dane leżą w katalogu roboczym aplikacji, więc kopia zapasowa aplikacji jest jednocześnie kopią bazy.

## Jak się do niej połączyć

Baza słucha wewnątrz kontenera na porcie **27017**.

**Z innej aplikacji na tym samym Node:**

1. Otwórz tamtą aplikację → zakładka **Porty** → karta **Połączenia**.
2. Wybierz aplikację MongoDB i kliknij **Połącz**.
3. W jej konfiguracji podaj adres w postaci:

```
mongodb://LOGIN:HASŁO@NAZWA-APLIKACJI:27017/NAZWA-BAZY?authSource=admin
```

`NAZWA-APLIKACJI` to nazwa aplikacji MongoDB **zapisana małymi literami**, ze spacjami zamienionymi na myślniki. `Moja Baza` to `moja-baza`.

Bez kroków 1–2 nic się nie połączy: kontenery nie widzą się nawzajem, dopóki połączenia nie przyznasz.

**Z zewnątrz, np. własnym klientem** — dodaj port w zakładce **Porty**: wewnętrzny `27017`, zewnętrzny dowolny wolny, dostęp **Vibe Network**. Publiczna MongoDB bez hasła jest przeszukiwana i kasowana przez boty w ciągu godzin — zdarzało się to na masową skalę.

## Konsola komend

Na zakładce **Przegląd** aplikacji jest **Konsola komend**. Wpisujesz wyrażenie `mongosh`, dostajesz odpowiedź.

1. Otwórz aplikację → zakładka **Przegląd**.
2. Wpisz polecenie, np. `db.getMongo().getDBNames()` albo `db.getSiblingDB("sklep").uzytkownicy.countDocuments()`.
3. Enter albo **Wykonaj**.

Strzałki w górę i w dół przewijają wcześniejsze polecenia.

Login i hasło administratora podstawiane są **wewnątrz kontenera**, ze zmiennych środowiskowych, które ten kontener już ma. Nie trafiają do żadnego polecenia uruchamianego przez VibeSSH, więc nie widać ich w liście procesów.

Każde polecenie to osobne uruchomienie klienta, więc **`use nazwa-bazy` nie przenosi się dalej**. Zamiast tego użyj `db.getSiblingDB("nazwa-bazy")` w tym samym poleceniu.

Aplikacja musi działać — w zatrzymanym kontenerze nie ma do czego się podłączyć.

## Częste problemy

**`Authentication failed`.** Najczęściej brak `?authSource=admin` w adresie połączenia. Konto z `MONGO_INITDB_ROOT_USERNAME` powstaje w bazie `admin`, a nie w tej, do której się łączysz.

**Baza wpuszcza bez hasła.** Zmienne zostały ustawione dopiero po pierwszym uruchomieniu. Obie działają wyłącznie przy **pierwszym** starcie, kiedy powstają pliki bazy. Konto załóż wtedy poleceniem `db.createUser` z poziomu klienta.

**Aplikacja nie może dojść do bazy.** Sprawdź po kolei: połączenie w karcie **Połączenia**, adres jako nazwa aplikacji małymi literami, port `27017` — ten wewnętrzny, nie ten opublikowany na zewnątrz.

**Kontener startuje i gaśnie.** Zajrzyj w **Logi**. Jeśli katalog roboczy zawiera już dane z innej, nowszej wersji MongoDB, starsza wersja odmówi ich otwarcia.
