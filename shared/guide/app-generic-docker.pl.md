---
id: app-generic-docker
title: Kontener Docker
section: blueprints
route: /applications
order: 150
---

Uruchamia dowolny obraz Dockera. To wyjście awaryjne na wszystko, co nie ma własnego rodzaju aplikacji w VibeSSH — nginx, WordPress, Grafana, cokolwiek znajdziesz na Docker Hubie.

Ten rodzaj dostają też serwery **przejęte** przez funkcję adopcji: VibeSSH nie podmienia wtedy niczego w katalogu, tylko uruchamia to, co już tam jest.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**, wybierz lokalizację i nazwę.
2. Wybierz **Kontener Docker (ogólny)** z listy rodzajów.
3. **Obraz** — pełna nazwa z tagiem, np. `nginx:latest` albo `itzg/minecraft-server:latest`.
4. **Nadpisanie polecenia** — zostaw puste, żeby obraz uruchomił się tak, jak go napisano. Wypełnij tylko wtedy, gdy chcesz zastąpić jego własny start; jeden argument na wiersz.
5. Utwórz aplikację i kliknij **Uruchom**.

## Zmienne środowiskowe

Większość obrazów konfiguruje się właśnie nimi. Ich nazwy znajdziesz na stronie obrazu na Docker Hubie. Wpisujesz je w **Ustawieniach → Środowisko**; hasła i klucze zaznacz jako **Sekret**.

## Porty

Nic nie jest publikowane automatycznie. Sprawdź na stronie obrazu, na którym porcie nasłuchuje (nginx — `80`, phpMyAdmin — `80`, Grafana — `3000`) i dodaj port w zakładce **Porty**: wewnętrzny to ten z opisu obrazu, zewnętrzny wybierasz sam.

## Dane i katalog roboczy

Katalog roboczy aplikacji jest podmontowany do kontenera. Wszystko, co program tam zapisze, przetrwa restart i odtworzenie kontenera. To, co zapisze gdzie indziej w kontenerze, przepadnie przy **Odtwórz kontener**.

**Gdzie ten katalog widać w kontenerze** zależy od tego, gdzie aplikacja działa:

- **Na serwerze** — pod tą samą ścieżką co na serwerze, na przykład `/home/container/mojaapka`.
- **Na Twoim komputerze** — pod `/home/container`. Katalog na dysku zostaje tam, gdzie
  jest (`C:\Users\...`), ale kontener Linuksa nie potrafi mieć ścieżki z literą dysku,
  więc w środku widzi go pod ścieżką linuksową.

Ma to znaczenie tylko wtedy, gdy sam wpisujesz ścieżki bezwzględne w komendzie albo
w zmiennych środowiskowych. Nazwy plików względem katalogu roboczego działają tak samo
w obu przypadkach — i dlatego blueprinty z nich korzystają.

## Docker na tym komputerze

Wybierając **Ten komputer** i runtime **Kontener Docker**, potrzebujesz zainstalowanego
Docker Desktopa (na Windowsie razem z WSL2). VibeSSH sprawdza to przy tworzeniu aplikacji
i mówi, jeśli go nie widzi.

Kilka rzeczy działa inaczej niż na serwerze i tak ma być:

- **Nie ma osobnego konta systemowego dla aplikacji.** Izolacja przez konta i `chown` to
  mechanizm POSIX, którego lokalny Docker Desktop nie ma. Nie jest to strata: pliki
  aplikacji są po prostu Twoje.
- **Konsola działa inaczej pod spodem**, ale w oknie wygląda tak samo.

**Naprawione w 0.1.0-beta.17.** We wcześniejszych wersjach lokalna aplikacja Dockera dawała
się utworzyć, a potem nie chciała wystartować — z komunikatem
`internal error: DockerRuntime requires a connection`, czyli o SSH, mimo wyboru
„Ten komputer". Dotyczyło to wszystkich aplikacji javowych (Paper, Velocity, Waterfall
i pozostałych), bo każda z nich prosi o osobne konto systemowe, którego lokalnie nie ma.
Poprawione: uruchamianie, restart i **Odtwórz kontener** działają lokalnie. Jeśli wciąż
widzisz ten komunikat — zaktualizuj aplikację.

## Zmiana obrazu na coś zarządzanego

Jeśli okaże się, że kontener zawiera zwykły serwer Paper, możesz przełączyć aplikację na rodzaj **Paper** w **Ustawieniach → Typ aplikacji**. Od tej pory VibeSSH zarządza wersją serwera — i pobierze wtedy własny plik serwera do tego katalogu. Ostrzeżenie przy przełączaniu mówi dokładnie, co się stanie.

## Częste problemy

**`no such image` albo błąd pobierania.** Literówka w nazwie obrazu, albo obraz jest prywatny — dane logowania do rejestru dodaje się w **Ustawieniach** aplikacji.

**Kontener startuje i gaśnie.** Zajrzyj w **Logi**. Zwykle brakuje wymaganej zmiennej środowiskowej opisanej na stronie obrazu.
