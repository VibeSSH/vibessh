---
id: actions
title: Akcje
section: nodes
route: /actions
order: 120
---

Akcje pozwalają zarządzać usługami systemowymi i kontenerami na serwerze — tymi, których nie utworzyłeś jako aplikacje w VibeSSH.

![Usługi systemd i kontenery Dockera na węźle](images/actions.png)

## Jak otworzyć

1. Otwórz **Akcje** w menu bocznym.
2. Wybierz serwer.

## Jak zarządzać usługą

1. Znajdź usługę na liście **Usługi systemd**. Użyj pola **Filtruj po nazwie**.
2. Kliknij ikonę akcji przy usłudze: uruchom, zatrzymaj, zrestartuj.
3. Aby usługa wstawała po restarcie serwera, użyj akcji włączenia.

## Dwa niezależne stany

| Stan | Co znaczy |
| --- | --- |
| **Aktywna** / **Nieaktywna** | Czy usługa działa w tej chwili. |
| **Włączona** / **Wyłączona** | Czy wstanie po restarcie serwera. |

Usługa może działać i nie być włączona — po restarcie serwera zniknie.

## Kontenery Docker

Sekcja **Kontenery Docker** wypisuje kontenery na serwerze. Możesz je uruchomić, zatrzymać, zrestartować i podejrzeć logi.

Aplikacjami utworzonymi w VibeSSH zarządzaj w module **Aplikacje**, nie tutaj.

## Jak sprawdzić, czy działa

- Usługa zmienia stan na **Aktywna**.
- Kontener zmienia stan na działający.

## Najczęstsze problemy

- **Brak kontenerów** — na serwerze nie ma Dockera albo konto nie ma do niego dostępu.
- **Usługa działa, ale po restarcie serwera jej nie ma** — była aktywna, ale nie włączona.
- **Nie mogę zatrzymać usługi** — konto nie ma uprawnień `sudo`.

## Więcej informacji

Zatrzymanie usługi zatrzymuje też wszystko, co od niej zależy. Nie zatrzymuj usługi `ssh` — stracisz dostęp do serwera z VibeSSH.
