---
id: app-phpmyadmin
title: phpMyAdmin
section: blueprints
route: /applications
order: 220
---

phpMyAdmin to strona w przeglądarce, na której przeglądasz i edytujesz zawartość bazy MySQL/MariaDB — tabele, rekordy, zapytania SQL, import i eksport.

Sam nic nie przechowuje. Jest tylko interfejsem do bazy, którą musisz mu wskazać, i to wskazanie jest jedyną rzeczą, którą trzeba tu zrobić dobrze.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**.
2. Na liście **Szablony** wybierz **phpMyAdmin do aplikacji MariaDB**.
3. Wybierz lokalizację — **tę samą, na której stoi baza**. phpMyAdmin na Twoim komputerze nie sięgnie do bazy na Node i odwrotnie.
4. Nadaj nazwę, np. `phpmyadmin`.
5. W kroku z ustawieniami zobaczysz pole **Baza danych**. Wybierz z listy tę, którą chcesz obsługiwać. Lista pokazuje aplikacje MariaDB oraz hosty baz danych z tej lokalizacji.
6. Utwórz aplikację i kliknij **Uruchom**.

Pole **Baza danych** robi trzy rzeczy naraz, których wcześniej trzeba było się domyślić: wpisuje `PMA_HOST`, wpisuje `PMA_PORT` i — przy aplikacji MariaDB — przyznaje połączenie między kontenerami. Bez tego trzeciego kroku żaden adres nie zadziała, bo kontenery nie widzą się nawzajem.

## Jak otworzyć panel

phpMyAdmin nasłuchuje wewnątrz kontenera na porcie **80**. Żeby wejść na niego przeglądarką, trzeba ten port opublikować:

1. Otwórz aplikację → zakładka **Porty** → **Dodaj port**.
2. **Port wewnętrzny**: `80`. **Port zewnętrzny**: dowolny wolny, np. `8080`.
3. **Dostęp sieciowy**: wybierz **Vibe Network**, jeśli tylko możesz.
4. Kliknij **Zsynchronizuj firewall**.
5. Wejdź na `http://ADRES-NODE:8080`.

Nie używaj portu **443**, jeśli nie masz certyfikatu. phpMyAdmin uzna wtedy, że działa po HTTPS, ustawi bezpieczne ciasteczko sesji, przeglądarka je odrzuci i zobaczysz *Failed to set session cookie*.

Zastanów się dwa razy przed ustawieniem **Publiczny**. To panel administracyjny bazy, chroniony wyłącznie hasłem do niej.

## Jak się zalogować

Loginem i hasłem **do bazy**, nie do VibeSSH.

- Baza z aplikacji MariaDB: `root` i `MYSQL_ROOT_PASSWORD`, albo konto z `MYSQL_USER` / `MYSQL_PASSWORD`.
- Baza z zakładki **Bazy danych**: kliknij przy niej ikonę oka — zobaczysz wygenerowany login i hasło.

## Częste problemy

**„getaddrinfo for db failed" albo inna nazwa, której nie ma.** `PMA_HOST` wskazuje na hosta, który nie istnieje — najczęściej wpisane `db` z jakiegoś poradnika o docker-compose. Popraw w **Ustawieniach → Środowisko**: dla aplikacji MariaDB to jej nazwa małymi literami, dla hosta baz danych adres widoczny w zakładce **Bazy danych** po znaku `@`.

**Logowanie kręci się i kończy timeoutem.** Dane są dobre, ale pakiet nie dochodzi. Jeśli baza jest hostem baz danych na Node, otwórz **Bazy danych** i kliknij przy nim ikonę odświeżenia — **Napraw dostęp z kontenerów**. Ustawia to serwer bazy tak, żeby słuchał na mostku Dockera, i dokłada regułę firewalla zawężoną do tego mostka. Jeśli baza jest aplikacją MariaDB, sprawdź kartę **Połączenia**.

**„Failed to set session cookie".** Patrz wyżej — port 443 bez HTTPS, albo zmienna `PMA_ABSOLUTE_URI` ustawiona na inny adres niż ten, którego używasz. Jeśli ją masz, wpisz w niej dokładnie ten adres z paska przeglądarki, ze schematem, portem i końcowym ukośnikiem.

**Chcesz podać adres bazy dopiero przy logowaniu.** Użyj szablonu **phpMyAdmin do dowolnego serwera**. Ustawia `PMA_ARBITRARY=1`, przez co na stronie logowania pojawia się dodatkowe pole na adres serwera. Przydaje się do baz spoza VibeSSH.
