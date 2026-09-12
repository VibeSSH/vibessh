---
id: glossary
title: Słowniczek
section: getting-started
route: /guide
order: 4
---

Słowa z interfejsu i z tego poradnika, które nie tłumaczą się same. Po dwa zdania każde — na tyle, żeby wiedzieć, co widzisz na ekranie.

Jeśli szukasz podstaw — czym jest Node, aplikacja, port albo firewall — te są w **Podstawowych pojęciach**. Tutaj są słowa z sieci prywatnej i firewalla, które padają w interfejsie bez wyjaśnienia.

## Blueprint

Szablon mówiący, **czym** jest aplikacja: jaki obraz uruchomić albo jaki plik wykonać, jakie pola pokazać przy tworzeniu i które zakładki mają sens. Paper, MariaDB i Redis to trzy różne blueprinty.

## Bind address (adres nasłuchu)

Adres sieciowy, na którym program **nasłuchuje** — czyli skąd w ogóle da się do niego połączyć, zanim jeszcze wejdzie w grę firewall.

`127.0.0.1` znaczy „tylko z tej samej maszyny". `0.0.0.0` znaczy „ze wszystkich adresów, jakie ta maszyna ma". To dlatego port opublikowany jako `0.0.0.0` jest dostępny z internetu, dopóki firewall go nie ograniczy — i dlatego przy porcie w zakładce **Porty** jest odznaka mówiąca, czy ograniczenie faktycznie działa.

## Port wewnętrzny i zewnętrzny

**Wewnętrzny** to port, na którym program nasłuchuje w środku swojego kontenera. **Zewnętrzny** to port, pod którym widać go z zewnątrz Node'a.

Zwykle są takie same, ale nie muszą: MariaDB może słuchać wewnątrz na `3306`, a być wystawiona na świat na `3307`. Reguły firewalla dotyczą tego zewnętrznego, bo to on jest osiągalny.

## Peer

Drugi koniec tunelu WireGuard. W Vibe Network każdy Node jest peerem dla każdego innego — jeśli masz trzy Node'y, każdy z nich ma dwóch peerów.

## Endpoint

Adres i port, pod którym peer jest osiągalny **z zewnątrz**, czyli z publicznego internetu — na przykład `203.0.113.10:51820`. WireGuard musi go znać, żeby wiedzieć, dokąd wysłać pierwszy pakiet.

Nie myl go z adresem `10.77.0.x`: endpoint to droga *do* tunelu, a `10.77.0.x` to adres *w* tunelu.

## Handshake

Wymiana potwierdzająca, że dwa peery naprawdę się dogadały. **To jest jedyny dowód, że tunel działa** — przydzielony adres i zapisana konfiguracja nic nie znaczą, dopóki nie ma handshake'u.

WireGuard odnawia go co jakiś czas, więc „ostatni handshake" sprzed kilkudziesięciu sekund to stan normalny, a nie problem.

## Reconcile (uzgodnienie)

Doprowadzenie stanu na serwerze do tego, co jest zapisane w VibeSSH. Uzgodnienie **nie** dopisuje wszystkiego od nowa — porównuje jedno z drugim i zmienia tylko różnice, więc można je uruchamiać wielokrotnie bez szkody.

Tak działa **Synchronizuj sieć** i **Zsynchronizuj firewall**. Dlatego obie te akcje można bezpiecznie kliknąć drugi raz, gdy nie masz pewności, czy pierwszy przeszedł.

## Source CIDR

Zapis mówiący, **z jakich adresów** wolno się łączyć. `10.77.0.0/16` znaczy „tylko z Vibe Network", a brak takiego zapisu znaczy „skądkolwiek".

To jest różnica między portem widocznym dla Twoich serwerów a portem widocznym dla całego internetu.

## Widoczność portu

Ustawienie, które wybierasz przy porcie, a z którego VibeSSH wyprowadza i adres nasłuchu, i regułę firewalla:

- **Publiczny** — cały internet.
- **Tylko Vibe Network** — wyłącznie Twoje Node'y w prywatnej sieci.
- **Tylko localhost** — wyłącznie ta sama maszyna.
- **Własny** — adres, który wpisujesz sam.

## Trust On First Use

Zasada, według której VibeSSH sprawdza klucz hosta SSH: przy pierwszym połączeniu pokazuje odcisk palca i pyta, czy go zaakceptować, a potem **pilnuje, żeby się nie zmienił**.

Zmiana klucza może znaczyć, że serwer został postawiony od nowa — albo że ktoś podstawił się w środku połączenia. Dlatego VibeSSH wtedy odmawia i czeka na Twoją świadomą decyzję.
