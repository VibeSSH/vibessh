---
id: app-redis
title: Redis
section: blueprints
route: /applications
order: 230
---

Redis to bardzo szybka pamięć podręczna i baza typu klucz–wartość. Używają go boty do sesji, panele do kolejek zadań, sieci serwerów do synchronizacji między instancjami.

Trzyma dane głównie w pamięci RAM. Nie jest zamiennikiem MariaDB — nadaje się do rzeczy, które można stracić przy restarcie, i do takich, które muszą być odczytane w ułamku milisekundy.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**, wybierz lokalizację i nazwę, np. `redis`.
2. Wybierz **Redis** z listy rodzajów.
3. **Wersja Redis** — zostaw `7` albo wybierz `8`.
4. **Hasło** — ustaw. Puste pole oznacza serwer bez żadnego uwierzytelniania.
5. Utwórz aplikację i kliknij **Uruchom**.

## Hasło i dostęp

Redis bez hasła jest bezpieczny **wyłącznie** wtedy, gdy jego port nigdzie nie jest opublikowany. Bot w innym kontenerze i tak go nie dosięgnie bez przyznanego połączenia, więc pusty wpis nie jest sam w sobie dziurą — staje się nią w chwili, w której opublikujesz port.

Jeśli publikujesz port na zewnątrz, ustaw hasło i wybierz dostęp **Vibe Network**. Redis wystawiony publicznie bez hasła jest przejmowany automatycznie, w ciągu minut.

## Jak połączyć się z niego z innej aplikacji

1. Otwórz aplikację, która ma korzystać z Redisa → zakładka **Porty** → karta **Połączenia**.
2. Połącz ją z aplikacją Redis.
3. W konfiguracji tej aplikacji podaj adres: **nazwa aplikacji Redis małymi literami**, port `6379`.

## Konsola komend

Na zakładce **Przegląd** aplikacji jest **Konsola komend**. Wpisujesz polecenie Redisa, dostajesz odpowiedź — bez łączenia się po SSH i bez szukania `redis-cli`.

1. Otwórz aplikację → zakładka **Przegląd**.
2. Wpisz polecenie, np. `KEYS *`, `GET klucz`, `INFO memory`.
3. Enter albo **Wykonaj**.

Strzałki w górę i w dół przewijają wcześniejsze polecenia.

Hasło podawane jest samo: VibeSSH odczytuje je **wewnątrz kontenera**, z argumentów działającego serwera, więc nie pojawia się w żadnym poleceniu uruchamianym na Twojej maszynie ani na nodzie.

Każde polecenie wykonuje się osobno. Do zwykłych operacji nie robi to różnicy; `SELECT 1` nie przeniesie się jednak na następne polecenie.

Aplikacja musi działać — `docker exec` nie ma się do czego podłączyć w zatrzymanym kontenerze.

## Częste problemy

**`NOAUTH Authentication required`.** Ustawiłeś hasło, a aplikacja go nie podaje. Dopisz je w jej konfiguracji.

**Połączenie odrzucone.** Brak połączenia w karcie **Połączenia**, albo zły adres — to nazwa aplikacji, nie `localhost`.

**Dane znikają po restarcie.** Tak działa Redis w tej konfiguracji. Do rzeczy, które muszą przetrwać, użyj bazy MariaDB.
