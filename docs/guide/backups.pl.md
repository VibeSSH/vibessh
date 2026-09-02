---
id: backups
title: Backupy
section: applications
route: /applications
order: 50
---

Backup to spakowana kopia katalogu roboczego aplikacji — świata Minecrafta, konfiguracji, wtyczek. Nie jest to obraz kontenera ani migawka całego węzła; kontener można odtworzyć w każdej chwili, dane w nim nie.

## Gdzie to jest

Aplikacja → zakładka **Backupy**. Lista backupów, przycisk **Utwórz backup teraz** i karta **Automatyczne backupy** pod nią.

## Backup ręczny

Jedno kliknięcie, jeden archiwum. Na liście backup ma znacznik **Ręczny** albo **Automatyczny**, datę i rozmiar.

Backup można zrobić na działającej aplikacji, ale warto wiedzieć, co to znaczy: pliki są kopiowane w trakcie, gdy proces do nich pisze. Dla serwera Minecraft praktyka jest taka, że przed backupem wykonuje się w konsoli `save-all` i `save-off`, a po nim `save-on`. Najpewniejszy backup to backup zatrzymanej aplikacji.

## Automatyczne backupy

| Ustawienie | Co robi |
| --- | --- |
| Twórz backupy automatycznie | Włącza harmonogram dla tej aplikacji. |
| Co ile godzin | Odstęp między backupami. |
| Ile ostatnich zachować | Zawsze zostaje tyle najnowszych archiwów. |
| Maks. wiek w dniach | Starsze są usuwane. Puste = bez limitu. |
| Maks. łączny rozmiar w MB | Najstarsze są usuwane, aż suma zejdzie poniżej. Puste = bez limitu. |

Limity działają razem: **backup jest usuwany, gdy przekroczy którykolwiek z nich**. Ustawienie „zachowaj 10" i „maks. 7 dni" oznacza, że backup starszy niż tydzień zniknie, nawet jeśli jest wśród dziesięciu najnowszych.

> Harmonogram działa tylko wtedy, gdy VibeSSH jest otwarty. Nie ma tu procesu w tle ani demona na węźle — jeśli aplikacja desktopowa jest zamknięta, backup o zaplanowanej porze się nie wykona. Po ponownym otwarciu zaległe backupy są nadrabiane.

## Przywracanie

Przywrócenie **nadpisuje obecne pliki** w katalogu roboczym i nie da się tego cofnąć. Dlatego przycisk jest zablokowany, dopóki aplikacja działa — przywracanie plików pod działającym procesem daje stan, którego nie miał ani backup, ani to, co było wcześniej.

Kolejność jest więc taka: zatrzymaj aplikację, przywróć, uruchom.

## Pobieranie

Pobiera archiwum na ten komputer. Przydaje się, gdy chcesz mieć kopię poza węzłem albo przenieść dane gdzie indziej.

## Magazyn zewnętrzny (S3)

Backup oznaczony jako **Wysłano do zewnętrznego magazynu** został dodatkowo skopiowany do skonfigurowanego magazynu zgodnego z S3. Cel konfiguruje się w ustawieniach aplikacji, nie tutaj.

Backup leżący wyłącznie na tym samym węźle co aplikacja chroni przed Twoją pomyłką, ale nie przed utratą węzła. Jeśli dane mają przetrwać awarię serwera, potrzebna jest kopia poza nim.

## Częste pomyłki

- **Harmonogram ustawiony, backupów nie ma** — sprawdź, czy VibeSSH był otwarty o tej porze. To jedyny wykonawca harmonogramu.
- **Backup zajmuje więcej, niż się spodziewasz** — świat Minecrafta rośnie wraz z eksploracją. Limit łącznego rozmiaru jest tu skuteczniejszy niż limit liczby.
- **Przywróciłem i wróciło coś innego, niż myślałem** — data na liście to moment utworzenia backupu, a nie moment ostatniego zapisu świata przez serwer.
