---
id: firewall
title: Firewall
section: nodes
route: /firewall
order: 70
---

Firewall to miejsce, w którym poziomy dostępu zadeklarowane przy portach aplikacji stają się prawdziwymi regułami na węźle. Bez niego „tylko Vibe Network" jest opisem intencji, a nie ograniczeniem.

## Gdzie to jest

Menu boczne → **Firewall**, po wybraniu serwera.

## Stan

Dwa pola, które trzeba czytać razem:

- **Backend** — który firewall wykryto na węźle (`ufw`). „brak" oznacza, że nie ma czym egzekwować reguł.
- **Egzekwowanie** — czy ten firewall jest **aktywny**. Backend obecny, ale nieaktywny, to najgorszy przypadek: reguły są zapisane i nic ich nie wymusza.

Przycisk **Zabezpiecz** włącza firewall — i robi to w kolejności, która nie odcina Ci dostępu: reguła dla SSH powstaje przed włączeniem egzekwowania.

## Skąd biorą się reguły

Lista pokazuje przy każdej regule jej pochodzenie:

| Pochodzenie | Znaczenie |
| --- | --- |
| SSH (zawsze dozwolone) | Port, przez który VibeSSH łączy się z węzłem. Zawsze otwarty — inaczej stracisz dostęp. |
| WireGuard (Vibe Network) | Port tunelu prywatnej sieci. |
| `nazwa` — port „…” | Wyliczona z portu aplikacji i jego poziomu dostępu. |
| Reguła ręczna | Dodana tutaj przez Ciebie. |

Trzy pierwsze rodzaje są **wyliczane, a nie pamiętane**. Synchronizacja liczy je od nowa z aktualnego stanu aplikacji i nakłada w całości, usuwając to, co przestało być potrzebne. Dlatego edytowanie ich ręcznie na węźle nie ma sensu — najbliższa synchronizacja i tak przywróci stan wyliczony.

## Reguły ręczne

**Dodaj regułę** otwiera port, którego nie zadeklarowała żadna aplikacja — na przykład na czas debugowania.

| Pole | Znaczenie |
| --- | --- |
| Etykieta | Opis dla Ciebie, żeby za tydzień wiedzieć, po co ta reguła. |
| Port | Numer portu (1–65535). |
| Protokół | TCP albo UDP. |
| Ogranicz do konkretnej sieci | Po zaznaczeniu reguła obowiązuje tylko dla podanego zakresu. |
| Zakres źródłowy (CIDR) | Np. `203.0.113.0/24`. |

Ograniczenie do zakresu jest tym, co odróżnia „otworzyłem port dla siebie" od „otworzyłem port dla internetu". Jeśli znasz swój adres, użyj go.

## Synchronizuj teraz

Nakłada wyliczony zestaw reguł. To samo robi **Zsynchronizuj firewall** na zakładce Porty aplikacji i pełna synchronizacja Vibe Network — trzy drogi do tej samej operacji.

## Częste pomyłki

- **Port jest otwarty w VibeSSH, a i tak niedostępny** — dostawcy VPS często mają własny firewall przed maszyną. VibeSSH go nie widzi i nie zmieni.
- **Włączyłem firewall i straciłem SSH** — nie powinno się zdarzyć, bo reguła SSH powstaje pierwsza. Jeśli jednak łączysz się z innego portu niż ten skonfigurowany w VibeSSH, ten port nie jest chroniony tą gwarancją.
- **Usunąłem regułę aplikacji, a wróciła** — reguły aplikacji są wyliczane. Żeby zniknęła na stałe, zmień dostęp portu w aplikacji.
