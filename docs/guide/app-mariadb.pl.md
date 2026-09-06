---
id: app-mariadb
title: MariaDB
section: blueprints
route: /applications
order: 210
---

MariaDB to serwer bazy danych. Trzymają w nim dane wtyczki Minecraft, sklepy, panele i praktycznie każda aplikacja webowa.

Ta aplikacja to **własny serwer bazy w kontenerze**, z danymi w katalogu roboczym aplikacji. To coś innego niż **Bazy danych** w menu bocznym — tam VibeSSH zakłada bazy na serwerze zainstalowanym bezpośrednio na Node. Jeśli nie wiesz, czego chcesz, przeczytaj sekcję „Co wybrać" na dole.

## Zanim zaczniesz

Przygotuj sobie hasło dla użytkownika `root`. MariaDB **nie uruchomi się bez niego** — kontener wystartuje, wypisze błąd o inicjalizacji i zgaśnie. To najczęstszy powód, dla którego świeża baza „nie działa".

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**.
2. W pierwszym kroku, na liście **Szablony**, wybierz **MariaDB z hasłem roota**. Szablon wpisze za Ciebie nazwy zmiennych, których wymaga obraz — nie musisz ich znać na pamięć.
3. Wybierz lokalizację (ten komputer albo Node) i nadaj nazwę, np. `baza`.
4. **Wersja MariaDB** — zostaw `11`, chyba że masz powód wybrać starszą.
5. W kroku ze **Środowiskiem** uzupełnij wartości:
   - **MYSQL_ROOT_PASSWORD** — hasło administratora. Wymagane.
   - **MYSQL_DATABASE** — nazwa bazy, która ma powstać od razu, np. `app`.
   - **MYSQL_USER** i **MYSQL_PASSWORD** — konto zwykłego użytkownika dla tej bazy.
6. Utwórz aplikację i kliknij **Uruchom**.

Pierwszy start trwa dłużej niż kolejne — serwer zakłada wtedy swoje pliki. Sprawdź zakładkę **Logi**: linia `ready for connections` oznacza, że baza wstała.

## Jak się do niej połączyć

Baza słucha wewnątrz kontenera na porcie **3306**.

**Z innej aplikacji na tym samym Node** — na przykład z serwera Minecraft albo z phpMyAdmina:

1. Otwórz drugą aplikację → zakładka **Porty** → karta **Połączenia**.
2. Wybierz aplikację MariaDB i kliknij **Połącz**.
3. W konfiguracji tej drugiej aplikacji podaj jako adres bazy **nazwę aplikacji MariaDB zapisaną małymi literami**, ze spacjami zamienionymi na myślniki. `Moja Baza` to `moja-baza`. Port to `3306`.

Bez kroku 1–2 nic się nie połączy. Każdy kontener ma własną prywatną sieć i nie widzi pozostałych, dopóki połączenia nie przyznasz — to celowe, żeby jedna przejęta aplikacja nie sięgnęła do wszystkich innych.

**Z zewnątrz, np. własnym klientem SQL na laptopie** — dodaj port w zakładce **Porty**: port wewnętrzny `3306`, zewnętrzny dowolny wolny. Ustaw dostęp na **Vibe Network**, nie na publiczny. Baza wystawiona publicznie jest atakowana w ciągu godzin.

## Częste problemy

**Kontener startuje i od razu gaśnie.** Brakuje `MYSQL_ROOT_PASSWORD`. Dopisz zmienną w **Ustawieniach → Środowisko** i uruchom ponownie. Uwaga: zmienne `MYSQL_DATABASE`, `MYSQL_USER` i `MYSQL_PASSWORD` działają tylko przy **pierwszym** starcie, kiedy powstają pliki bazy. Później nowe konta zakłada się już w samej bazie.

**Druga aplikacja nie widzi bazy.** Sprawdź w kolejności: czy jest połączenie w karcie **Połączenia**, czy adres to nazwa aplikacji małymi literami, i czy port to `3306`, a nie ten opublikowany na zewnątrz.

**Zapomniałeś hasła roota.** Nie da się go odczytać — jest zapisane jako sekret i VibeSSH sam go nie zna. Ustaw nową wartość zmiennej i zmień hasło w bazie poleceniem `ALTER USER`.

## Co wybrać: ta aplikacja czy Bazy danych

Weź **aplikację MariaDB**, kiedy chcesz osobny, odizolowany serwer bazy — na przykład na jeden projekt, z własną wersją i własnym katalogiem, który da się wykonać kopią zapasową razem z resztą.

Weź **Bazy danych** z menu bocznego, kiedy chcesz jeden wspólny serwer na Node i zakładać na nim bazy dla wielu aplikacji jednym kliknięciem. VibeSSH generuje wtedy nazwę bazy, użytkownika i hasło, i sam pilnuje dostępu.
