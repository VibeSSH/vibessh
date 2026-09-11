---
id: ports
title: Porty
section: applications
route: /applications
order: 30
---

Port pozwala udostępnić usługę — na przykład serwer Minecraft graczom.

![Zakładka Porty: trzy porty z plakietkami poziomu dostępu i synchronizacja firewalla na dole karty](images/ports-tab.png)

## Jak dodać port

1. Otwórz aplikację → zakładka **Porty**.
2. Kliknij **Dodaj port**.
3. **Nazwa** — opis dla Ciebie, np. `Minecraft`.
4. **Protokół** — wybierz `TCP` albo `UDP`.
5. **Dostęp sieciowy** — wybierz jedną z opcji (opisane niżej).
6. **Port wewnętrzny** — numer, na którym nasłuchuje aplikacja, np. `25565`.
7. **Port zewnętrzny** — zostaw pusty, żeby użyć tego samego numeru.
8. Zapisz.
9. Kliknij **Zsynchronizuj firewall**.

## Dostęp sieciowy

| Opcja | Kto się połączy |
| --- | --- |
| **Publiczny** | Każdy z internetu. |
| **Tylko Vibe Network** | Tylko Twoje inne Node'y. |
| **Tylko localhost** | Tylko procesy na tym samym Node. |
| **Własny adres** | Podajesz adres ręcznie. |

## Przykłady

- Minecraft Java: `25565`, TCP, **Publiczny**.
- Minecraft Bedrock: `19132`, UDP, **Publiczny**.
- RCON: `25575`, TCP, **Tylko Vibe Network**.
- MariaDB: `3306`, TCP, **Tylko Vibe Network**.

Baz danych nie wystawiaj jako **Publiczny**, jeśli nie masz konkretnego powodu.

## Jak sprawdzić, czy działa

- Port jest na liście z właściwą plakietką dostępu.
- Gracze łączą się na adres IP serwera i ten numer portu.
- Po zmianie na działającej aplikacji kontener zostaje odtworzony automatycznie.

## Najczęstsze problemy

- **Port nie odpowiada z internetu** — ustaw **Publiczny**, kliknij **Zsynchronizuj firewall**, a potem sprawdź firewall w panelu dostawcy VPS. VibeSSH go nie widzi.
- **Port jest zajęty** — inna aplikacja używa tego numeru. Wpisz inny **Port zewnętrzny**.
- **Nie mogę usunąć portu** — port oznaczony jako **Wymagany** pochodzi z obrazu. Można go edytować, ale nie usunąć.

## Więcej informacji

Sam wybór dostępu to deklaracja; egzekwuje ją firewall Node'a, dlatego po zmianie trzeba kliknąć **Zsynchronizuj firewall**. Docker zapisuje porty w kontenerze przy jego tworzeniu, więc zmiana portu na działającej aplikacji odtwarza kontener. Pliki aplikacji pozostają nietknięte.
