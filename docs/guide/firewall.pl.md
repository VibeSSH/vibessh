---
id: firewall
title: Firewall
section: nodes
route: /firewall
order: 80
---

Firewall kontroluje, które porty Node są dostępne z internetu.

![Firewall węzła: backend ufw, egzekwowanie aktywne i cztery reguły](images/firewall.png)

## Jak włączyć firewall

1. Otwórz **Firewall** i wybierz serwer.
2. Sprawdź kartę **Stan**.
3. Jeśli **Egzekwowanie** jest **Nieaktywne**, kliknij **Zabezpiecz**.
4. Poczekaj, aż **Egzekwowanie** zmieni się na **Aktywne**.

Reguła dla SSH powstaje przed włączeniem firewalla, więc nie stracisz dostępu.

## Jak zobaczyć reguły

Karta **Reguły** wypisuje wszystkie otwarte porty. Przy każdym widać, skąd pochodzi:

| Pochodzenie | Znaczenie |
| --- | --- |
| **SSH (zawsze dozwolone)** | Port, przez który łączy się VibeSSH. |
| **WireGuard (Vibe Network)** | Port prywatnej sieci. |
| nazwa aplikacji — port | Wyliczone z portu aplikacji. |
| **Reguła ręczna** | Dodana przez Ciebie. |

## Jak dodać własną regułę

1. Kliknij **Dodaj regułę**.
2. **Etykieta** — opcjonalnie, np. `debugowanie`.
3. **Port** — numer portu.
4. **Protokół** — `TCP` albo `UDP`.
5. Aby ograniczyć dostęp, zaznacz **Ogranicz do konkretnej sieci** i wpisz **Zakres źródłowy (CIDR)**, np. `203.0.113.0/24`.
6. Zapisz.

## Jak usunąć regułę

1. Znajdź regułę z oznaczeniem **Reguła ręczna**.
2. Kliknij ikonę kosza.
3. Potwierdź.

Reguł aplikacji nie usuwa się tutaj. Zmień **Dostęp sieciowy** portu w zakładce **Porty** aplikacji.

## Jak sprawdzić, czy działa

- **Backend** pokazuje `ufw`.
- **Egzekwowanie** pokazuje **Aktywne**.
- Na liście są tylko te porty, które mają być otwarte.

## Najczęstsze problemy

- **Backend: brak** — na Node nie ma ufw. Zainstaluj go w konfiguracji Node'a.
- **Reguły nieegzekwowane** — firewall jest zainstalowany, ale wyłączony. Kliknij **Zabezpiecz**.
- **Port otwarty w VibeSSH, a i tak niedostępny** — sprawdź firewall w panelu dostawcy VPS.
- **Usunięta reguła aplikacji wróciła** — to normalne. Reguły aplikacji są wyliczane z ustawień portów.

## Więcej informacji

Aplikacje backendowe — bazy danych, panele administracyjne, RCON — powinny mieć dostęp **Tylko Vibe Network**, a nie **Publiczny**. Publiczny jest dla portów, na które łączą się gracze lub użytkownicy.
