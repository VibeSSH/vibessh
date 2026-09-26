---
id: schedules
title: Harmonogramy
section: applications
route: /applications
order: 55
---

Harmonogram to automatyczny restart, zatrzymanie albo start aplikacji o wybranej porze, na przykład restart serwera codziennie o 4:00.

## Jak dodać harmonogram

1. Otwórz aplikację i wejdź w zakładkę **Harmonogramy**.
2. Kliknij **Dodaj harmonogram**.
3. Wpisz **Nazwę**, na przykład `Nocny restart`.
4. W polu **Co zrobić** wybierz **Restart**, **Zatrzymanie** albo **Start**.
5. W polu **Kiedy** wybierz jedną z opcji:
   - **Codziennie** i godzinę, na przykład `04:00`,
   - **W wybrane dni** i zaznacz dni tygodnia,
   - **Co kilka godzin** i wybierz odstęp,
   - **Własny (cron)**, jeśli znasz zapis crona.
6. Sprawdź pod formularzem datę **Pierwszego uruchomienia**.
7. Kliknij **Zapisz**.

## Jak sprawdzić, czy działa

1. Przy harmonogramie kliknij ikonę **Uruchom teraz**.
2. Aplikacja wykona tę akcję od razu, tak samo jak zrobi to o zaplanowanej godzinie.
3. Pod harmonogramem pojawi się **Ostatnio:** z datą.

Przy każdym harmonogramie widać też, kiedy wykona się **następnie**.

## Jak wstrzymać albo usunąć harmonogram

- **Wstrzymanie:** wyłącz przełącznik przy harmonogramie. Zostaje na liście, ale się nie wykonuje. Włącz go ponownie, żeby wrócił.
- **Usunięcie:** kliknij ikonę kosza i potwierdź.
- **Zmiana:** kliknij ikonę edycji, popraw i zapisz.

## Ważne

- **Harmonogramy działają bez VibeSSH.** Wykonuje je sam Node, więc restart o 4:00 zadzieje się także przy wyłączonym komputerze. Backupy automatyczne działają inaczej: tylko przy otwartym VibeSSH.
- **Godziny są według zegara Node'a.** Jeśli Node jest w innej strefie czasowej niż Ty, obok godziny widać też Twój czas.
- **Serwer ma czas na zapis.** Przy restarcie i zatrzymaniu serwer dostaje do 2 minut na zapisanie świata, zanim zostanie wyłączony na siłę.

## Najczęstsze problemy

- **Nie ma zakładki Harmonogramy** - działa tylko dla aplikacji Docker na Nodzie połączonym przez SSH. Aplikacje uruchomione na tym komputerze i Node'y z Vibe Agentem jej nie mają.
- **„Ten Node nie ma crona”** - kliknij **Zainstaluj crona** pod komunikatem. VibeSSH zainstaluje go i od razu zapisze harmonogram. Możesz też zrobić to sam w terminalu Node'a: `sudo apt install cron`.
- **„Ostatnio nie powiodło się”** - najedź na ten napis, żeby zobaczyć powód. Najczęściej aplikacja była usunięta albo Docker nie działał.
- **Restart o złej godzinie** - sprawdź pod listą, w jakiej strefie czasowej jest Node.

## Więcej informacji

Harmonogram zapisuje się na Nodzie jako plik crona, a wykonuje go mały skrypt VibeSSH. Usunięcie aplikacji usuwa też jej harmonogramy z Node'a. Migracja aplikacji na inny Node przenosi harmonogramy razem z nią.
