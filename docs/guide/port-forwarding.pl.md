---
id: port-forwarding
title: Tunele SSH
section: nodes
route: /port-forwarding
order: 75
---

Tunel SSH przenosi ruch przez połączenie z węzłem. Służy do sięgnięcia po coś, co nie jest i nie ma być wystawione na świat — panel bazy danych, port administracyjny, usługa słuchająca tylko na localhoście węzła.

To osobna rzecz niż porty aplikacji i firewall. Tam deklarujesz, co ma być dostępne na stałe; tutaj otwierasz sobie przejście na chwilę.

## Gdzie to jest

Menu boczne → **Przekierowanie portów**, po wybraniu serwera.

> Tunele **nie są zapisywane**. Działają tylko dopóki VibeSSH jest otwarte, a po ponownym uruchomieniu trzeba je otworzyć na nowo. To celowe — trwały tunel to w praktyce otwarty port, o którym się zapomina.

## Trzy typy

**Lokalny** — port na Twoim komputerze prowadzi do adresu widzianego z węzła. Najczęstszy przypadek: `localhost:3306` u Ciebie wchodzi na `127.0.0.1:3306` na serwerze, i klient bazy łączy się tak, jakby baza była lokalna.

**Zdalny** — odwrotnie: port na serwerze prowadzi do adresu widzianego z Twojego komputera. Używane, gdy to serwer ma sięgnąć po coś u Ciebie.

**Dynamiczny (SOCKS5)** — proxy na Twoim komputerze, przez które ruch wychodzi z węzła. Nie wskazujesz jednego celu; wskazuje go aplikacja korzystająca z proxy.

## Pola

| Pole | Znaczenie | Przykład |
| --- | --- | --- |
| Adres nasłuchu | Gdzie tunel przyjmuje połączenia. `127.0.0.1` udostępnia go tylko Tobie. | `127.0.0.1` |
| Port nasłuchu | Numer, na którym słucha. `0` = dowolny wolny port. | `3306` |
| Adres docelowy | Dokąd prowadzi, widziany z drugiej strony. | `127.0.0.1` |
| Port docelowy | Port po drugiej stronie. | `3306` |

Adres docelowy dla tunelu lokalnego jest rozwiązywany **na węźle**, nie u Ciebie. Dlatego `127.0.0.1` oznacza tam „ten serwer", a nie Twój komputer — i dlatego to działa dla usług przywiązanych do localhosta węzła.

## Częste pomyłki

- **Wpisałem adres docelowy widziany z mojego komputera** — cel jest rozwiązywany po stronie węzła. Wpisz to, co widzi serwer.
- **Tunel zniknął** — zamknięcie VibeSSH zamyka wszystkie. Nic ich nie odtwarza.
- **Port nasłuchu zajęty** — coś na Twoim komputerze już go trzyma. Wpisz `0`, a system przydzieli wolny.
- **Chcę stałego dostępu** — to nie jest do tego. Do stałego dostępu służy port aplikacji z odpowiednim poziomem dostępu, najlepiej „Vibe Network".
