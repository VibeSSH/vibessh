---
id: backups
title: Backupy
section: applications
route: /applications
order: 50
---

Backup to kopia plików aplikacji: świata, konfiguracji i wtyczek.

![Lista backupów: automatyczne i ręczny, z rozmiarem i oznaczeniem wysłania do S3](images/backups-tab.png)

## Jak zrobić backup

1. Otwórz aplikację → zakładka **Backupy**.
2. Kliknij **Utwórz backup teraz**.
3. Poczekaj, aż backup pojawi się na liście.

Najpewniejszy backup robisz przy zatrzymanej aplikacji.

## Jak włączyć backupy automatyczne

1. W zakładce **Backupy** znajdź kartę **Automatyczne backupy**.
2. Zaznacz **Twórz backupy automatycznie**.
3. **Co ile godzin** — np. `24`.
4. **Ile ostatnich zachować** — np. `7`.
5. **Maks. wiek w dniach** — opcjonalnie, np. `30`.
6. **Maks. łączny rozmiar w MB** — opcjonalnie, np. `20480`.
7. Zapisz.

Backup jest usuwany, gdy przekroczy którykolwiek z ustawionych limitów.

## Jak przywrócić backup

1. Zatrzymaj aplikację przyciskiem **Zatrzymaj**.
2. Wejdź w zakładkę **Backupy**.
3. Kliknij ikonę przywracania przy wybranym backupie.
4. Potwierdź.
5. Uruchom aplikację przyciskiem **Uruchom**.

Przywracanie nadpisuje obecne pliki i nie da się go cofnąć. Przycisk jest nieaktywny, dopóki aplikacja działa.

## Jak pobrać backup

1. Kliknij ikonę pobierania przy backupie.
2. Wskaż miejsce zapisu na komputerze.

## Jak sprawdzić, czy działa

- Backup jest na liście z datą i rozmiarem.
- Ma oznaczenie **Ręczny** albo **Automatyczny**.
- Po wysłaniu do magazynu zewnętrznego widnieje **Wysłano do zewnętrznego magazynu (S3)**.

## Najczęstsze problemy

- **Harmonogram ustawiony, backupów nie ma** — backupy automatyczne działają tylko wtedy, gdy VibeSSH jest otwarte.
- **Backupy zajmują dużo miejsca** — świat Minecrafta rośnie. Ustaw **Maks. łączny rozmiar w MB**.
- **Backup obciąża serwer** — pakowanie dużego świata zajmuje dysk i procesor. Rób to poza godzinami szczytu.

## Więcej informacji

Backup obejmuje katalog roboczy aplikacji, nie zawartość baz danych. Cel zewnętrzny (S3) konfigurujesz w **Ustawienia → Cel backupów**. Backup leżący tylko na tym samym Node co aplikacja nie chroni przed utratą serwera.
