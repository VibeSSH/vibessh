---
id: app-postgres
title: PostgreSQL
section: blueprints
route: /applications
order: 215
---

PostgreSQL to serwer bazy danych. Wybierają go aplikacje webowe, boty i panele, które potrzebują czegoś mocniejszego niż plik — i jest to najczęstszy wybór poza światem Minecrafta.

Ta aplikacja to **własny serwer bazy w kontenerze**, z danymi w katalogu roboczym aplikacji. To coś innego niż **Bazy danych** w menu bocznym — tam VibeSSH zakłada bazy na serwerze zainstalowanym bezpośrednio na Node. Jeśli nie wiesz, czego chcesz, przeczytaj sekcję „Co wybrać" na dole.

## Zanim zaczniesz

Przygotuj hasło administratora. PostgreSQL **nie uruchomi się bez niego** — kontener wystartuje, wypisze błąd i zgaśnie.

Druga rzecz jest mniej oczywista i kosztowna, więc warto ją rozumieć, nawet jeśli szablon załatwi ją za Ciebie. Obraz PostgreSQL zakłada bazę tam, gdzie wskazuje zmienna **PGDATA**. Jeśli jej nie ustawisz, baza powstanie **wewnątrz kontenera** — będzie działać bez zarzutu, a zniknie przy pierwszym odtworzeniu kontenera, czyli choćby przy zmianie wersji. Dlatego `PGDATA` musi wskazywać katalog aplikacji.

Wartość to **sama kropka**: `PGDATA=.` — nie `./pgdata` ani żadna podścieżka. Obraz w momencie zakładania katalogu działa już jako użytkownik `postgres`, a katalog aplikacji nie należy do tego konta, więc podkatalogu nie utworzy i kontener się nie podniesie.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**.
2. W pierwszym kroku, na liście **Szablony**, wybierz **PostgreSQL z hasłem i katalogiem danych**. Szablon wpisze za Ciebie `PGDATA` i nazwy pozostałych zmiennych — nie musisz ich pamiętać.
3. Wybierz lokalizację (ten komputer albo Node) i nadaj nazwę, np. `baza`.
4. **Wersja PostgreSQL** — zostaw `17`, chyba że aplikacja, która ma z niej korzystać, wymaga starszej.
5. W kroku ze **Środowiskiem** uzupełnij wartości:
   - **POSTGRES_PASSWORD** — hasło administratora. Wymagane.
   - **POSTGRES_DB** — nazwa bazy, która ma powstać od razu, np. `app`.
   - **PGDATA** — zostaw `.` wpisane przez szablon.
6. Utwórz aplikację i kliknij **Uruchom**.

Pierwszy start trwa dłużej niż kolejne — serwer zakłada wtedy swoje pliki. Sprawdź zakładkę **Logi**: linia `database system is ready to accept connections` oznacza, że baza wstała.

Jeśli tworzysz aplikację bez szablonu, katalog roboczy musi być **pusty**. PostgreSQL odmawia założenia bazy w katalogu, w którym coś już leży.

## Jak się do niej połączyć

Baza słucha wewnątrz kontenera na porcie **5432**.

**Z innej aplikacji na tym samym Node:**

1. Otwórz drugą aplikację → zakładka **Porty** → karta **Połączenia**.
2. Wybierz aplikację PostgreSQL i kliknij **Połącz**.
3. W konfiguracji tej drugiej aplikacji podaj jako adres bazy **nazwę aplikacji PostgreSQL zapisaną małymi literami**, ze spacjami zamienionymi na myślniki. `Moja Baza` to `moja-baza`. Port to `5432`, użytkownik domyślnie `postgres`.

Bez kroku 1–2 nic się nie połączy. Każdy kontener ma własną prywatną sieć i nie widzi pozostałych, dopóki połączenia nie przyznasz — to celowe, żeby jedna przejęta aplikacja nie sięgnęła do wszystkich innych.

**Z zewnątrz, np. własnym klientem SQL na laptopie** — dodaj port w zakładce **Porty**: wewnętrzny `5432`, zewnętrzny dowolny wolny. Ustaw dostęp na **Vibe Network**, nie na publiczny. Baza wystawiona publicznie jest atakowana w ciągu godzin.

## Zapytania bez wychodzenia z aplikacji

Zakładka **Konsola** tej aplikacji uruchamia `psql` wewnątrz kontenera. Wpisz zapytanie, np. `SELECT version();` albo `\dt`, i zobaczysz odpowiedź serwera. Nie musisz znać hasła — konsola łączy się lokalnym gniazdem, któremu serwer ufa.

## Częste problemy

**Kontener startuje i od razu gaśnie.** Najczęściej brakuje `POSTGRES_PASSWORD`. Jeśli jest, a w logach widzisz `Permission denied` przy tworzeniu katalogu — `PGDATA` wskazuje podkatalog. Zmień na `.`.

**Dane zniknęły po zmianie wersji.** `PGDATA` nie było ustawione, więc baza mieszkała w kontenerze. Kontener odtworzony przy zmianie ustawień zaczyna od zera. Ustaw `PGDATA=.` i zakładaj bazę od nowa — poprzednich danych nie da się odzyskać, bo nigdy nie trafiły na dysk Node'a.

**„database files are incompatible with server".** Baza została założona starszą wersją PostgreSQL niż ta, którą teraz uruchamiasz. PostgreSQL nie podnosi formatu sam. Wróć do poprzedniej wersji w **Ustawieniach**, zrób zrzut przez `pg_dump`, a potem wczytaj go do nowej bazy.

**Zapomniałeś hasła.** Nie da się go odczytać — jest zapisane jako sekret i VibeSSH sam go nie zna. Zmień hasło w bazie z poziomu **Konsoli** poleceniem `ALTER USER postgres PASSWORD 'nowe';`, a potem zaktualizuj zmienną środowiskową, żeby się zgadzały.

## Co wybrać: ta aplikacja czy Bazy danych

Weź **aplikację PostgreSQL**, kiedy chcesz osobny, odizolowany serwer bazy — na jeden projekt, z własną wersją i własnym katalogiem, który da się wykonać kopią zapasową razem z resztą.

Weź **Bazy danych** z menu bocznego, kiedy chcesz jeden wspólny serwer na Node i zakładać na nim bazy dla wielu aplikacji jednym kliknięciem. VibeSSH generuje wtedy nazwę bazy, użytkownika i hasło, i sam pilnuje dostępu.
