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

## Zmiana obrazu na coś zarządzanego

Jeśli okaże się, że kontener zawiera zwykły serwer Paper, możesz przełączyć aplikację na rodzaj **Paper** w **Ustawieniach → Typ aplikacji**. Od tej pory VibeSSH zarządza wersją serwera — i pobierze wtedy własny plik serwera do tego katalogu. Ostrzeżenie przy przełączaniu mówi dokładnie, co się stanie.

## Częste problemy

**`no such image` albo błąd pobierania.** Literówka w nazwie obrazu, albo obraz jest prywatny — dane logowania do rejestru dodaje się w **Ustawieniach** aplikacji.

**Kontener startuje i gaśnie.** Zajrzyj w **Logi**. Zwykle brakuje wymaganej zmiennej środowiskowej opisanej na stronie obrazu.
