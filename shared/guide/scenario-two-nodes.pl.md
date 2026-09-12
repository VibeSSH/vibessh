---
id: scenario-two-nodes
title: Scenariusz - dwa serwery i baza tylko dla nich
section: getting-started
route: /servers
order: 3
---

Ten poradnik przechodzi jedną drogę od początku do końca: **dwa serwery, prywatna sieć między nimi i baza danych, do której dostanie się drugi serwer, a internet nie.**

To najczęstszy układ: serwer gry na jednej maszynie, baza na drugiej. Po drodze przy każdym kroku jest napisane, co masz zobaczyć — żebyś wiedział, czy iść dalej, czy się cofnąć.

Zajmuje około dwudziestu minut. Potrzebujesz dwóch serwerów z Linuksem i dostępu po SSH do obu.

## Krok 1. Dodaj pierwszy serwer

**Serwery** → **Dodaj serwer**. Wpisz adres, użytkownika i klucz albo hasło. Kliknij **Testuj połączenie** przed zapisaniem.

Przy pierwszym łączeniu zobaczysz odcisk palca klucza hosta i pytanie, czy go zaakceptować. To normalne — VibeSSH zapamiętuje go i od tej pory ostrzeże, jeśli się zmieni.

> **Co masz zobaczyć:** na liście pojawia się serwer, a przy nim zielona kropka i status **Online**. Jeśli jest **Offline**, sprawdź port SSH i firewall — osiągalność sprawdzana jest połączeniem TCP, nie pingiem.

## Krok 2. Dodaj drugi serwer

To samo co w kroku 1. Nazwij je tak, żebyś je rozróżniał — na przykład `gra` i `baza`.

> **Co masz zobaczyć:** dwa serwery na liście, oba **Online**.

## Krok 3. Połącz je prywatną siecią

**Vibe Network** → przy każdym serwerze **Dołącz do sieci**. Zrób to dla obu.

VibeSSH zainstaluje WireGuard, jeśli go nie ma, wygeneruje klucze i przydzieli każdemu serwerowi adres w sieci `10.77.0.0/16` — pierwszy dostanie `10.77.0.1`, drugi `10.77.0.2`.

Potem kliknij **Synchronizuj sieć**. To jest moment, w którym serwery dowiadują się o sobie nawzajem.

> **Co masz zobaczyć:** przy obu serwerach adres `10.77.0.x` i **ostatni handshake** sprzed kilku sekund. Handshake to dowód, że tunel naprawdę stoi — sam przydzielony adres jeszcze o niczym nie świadczy.

**Jeśli handshake się nie pojawia:** synchronizacja pokazuje wynik osobno dla każdego serwera, więc zobaczysz, który z nich zawiódł. Najczęstsza przyczyna to zablokowany port UDP WireGuarda — sprawdź, czy dostawca VPS-a nie filtruje ruchu UDP.

## Krok 4. Postaw bazę na drugim serwerze

**Aplikacje** → **Utwórz aplikację**. Wybierz serwer `baza`, a w sekcji **Zacznij od szablonu** kliknij **MariaDB z hasłem roota**.

Szablon wypełnia zmienne, bez których obraz MariaDB nie wstanie. Bez niego kontener powstanie i od razu się zatrzyma — to była najczęstsza przyczyna zgłoszeń.

> **Co masz zobaczyć:** aplikacja ze statusem **Działa**. Jeśli **Zatrzymana**, zajrzyj w **Logi** — MariaDB pisze tam wprost, czego jej brakuje.

## Krok 5. Otwórz port bazy tylko dla prywatnej sieci

W aplikacji bazy: zakładka **Porty** → **Dodaj port**. Port `3306`, protokół TCP, a jako widoczność wybierz **Tylko Vibe Network**.

To jest krok, w którym decyduje się bezpieczeństwo całego układu. **Publiczny** znaczy „cały internet".

> **Co masz zobaczyć:** przy porcie dwie odznaki — **Tylko Vibe Network** i zielone **Chroniony**. Zielone „Chroniony" znaczy, że na tym serwerze działa firewall i reguła dla tego portu naprawdę jest zastosowana.

> **Jeśli widzisz czerwone „Niechroniony"** — port jest w tej chwili otwarty na świat, mimo ustawionej widoczności. Kliknij **Zsynchronizuj firewall** pod listą. Jeśli po tym dalej jest czerwony, to znaczy, że na serwerze nie ma ufw albo jest wyłączony; komunikat pod przyciskiem powie, który to przypadek.

## Krok 6. Utwórz bazę dla aplikacji

Zakładka **Bazy danych** w aplikacji, która ma z niej korzystać → **Utwórz bazę**. Nazwa, login i hasło generują się same.

Kliknij ikonę oka, żeby zobaczyć dane połączenia. Znajdziesz tam trzy pola osobno: **Host**, **Port** i **Adres (host i port razem)** — bo część konfiguracji chce ich rozdzielonych, a część sklejonych.

> **Co masz zobaczyć:** wiersz z nazwą bazy i loginem. Hasło pokazuje się na żądanie i nie jest trzymane w zwykłej bazie aplikacji.

## Krok 7. Sprawdź, że działa — i że nie działa stamtąd, skąd nie powinno

To jest krok, którego nie pomijaj. Dwa sprawdzenia.

**Z serwera `gra`** (zakładka **Terminal**), gdzie `10.77.0.2` to adres serwera `baza` z kroku 3:

```
nc -zv 10.77.0.2 3306
```

Powinno napisać **succeeded** albo **open**.

**Ze swojego komputera**, na publiczny adres serwera `baza`:

```
nc -zv PUBLICZNY_ADRES_BAZY 3306
```

Powinno **przeciąć się na timeout**. To jest dobra wiadomość: znaczy, że firewall odrzuca pakiet po cichu.

> **Jeśli drugie sprawdzenie się połączy**, port jest otwarty na świat. Wróć do kroku 5 i sprawdź odznakę przy porcie.

> **Jeśli pierwsze sprawdzenie daje timeout, a drugie też** — sieć prywatna nie stoi. Wróć do kroku 3 i sprawdź handshake.

## Gdy działa połowa

Najczęstsze kombinacje i co one znaczą:

| Objaw | Co to znaczy | Gdzie szukać |
| --- | --- | --- |
| Handshake jest, `nc` z drugiego serwera daje timeout | Sieć stoi, firewall bazy nie wpuszcza | Krok 5 — odznaka przy porcie |
| Handshake jest, aplikacja nie łączy się do bazy po nazwie | DNS prywatny nie zsynchronizowany | **Vibe Network** → **Synchronizuj DNS** |
| Brak handshake'u tylko na jednym serwerze | Ten serwer nie wypuszcza albo nie przyjmuje UDP | Firewall dostawcy VPS-a, nie VibeSSH |
| Wszystko zielone, aplikacja dalej nie łączy się | Zły adres w konfiguracji aplikacji | Zakładka **Bazy danych** → dane połączenia, pola **Host** i **Port** osobno |

## Co dalej

Ten sam układ rozszerza się bez zmiany zasad: trzeci serwer dołącza do sieci tak samo, a kolejna baza dostaje port z tą samą widocznością.

Jeśli aplikacje mają się widzieć po nazwach zamiast po adresach IP, zajrzyj do poradnika **Vibe Network** — sekcja o aliasach DNS.
