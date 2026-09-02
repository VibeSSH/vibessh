---
id: getting-started
title: Pierwsze kroki
section: getting-started
route: /
order: 1
---

Od pustej aplikacji do działającego serwera prowadzi pięć kroków. Każdy z nich ma tu własny rozdział; ten pokazuje kolejność i to, co wynika z jednego kroku dla następnego.

## 1. Dodaj serwer

**Serwery → Dodaj serwer**. Adres, użytkownik, hasło albo klucz. Użyj **Testuj połączenie**, zanim zapiszesz — otwiera prawdziwe połączenie i nic nie zapisuje, więc poprawki nic nie kosztują.

Konto powinno mieć `sudo`. Bez tego część rzeczy zadziała, ale instalacja Dockera, reguły firewalla i konta dedykowane dla aplikacji — nie.

## 2. Utwórz aplikację

**Aplikacje → Nowa aplikacja**, wybór blueprintu i węzła. Blueprint to punkt wyjścia: obraz, porty, pliki konfiguracyjne. Wszystko da się potem zmienić.

Po utworzeniu aplikacja jeszcze nie działa — uruchamiasz ją przyciskiem **Uruchom**.

## 3. Otwórz port

**Aplikacja → Porty**. Blueprint zwykle deklaruje port już przy tworzeniu; sprawdź, czy ma właściwy poziom dostępu.

Sam poziom dostępu to deklaracja. Egzekwuje ją firewall węzła, więc kliknij **Zsynchronizuj firewall**.

> Dostawcy VPS często mają własny firewall przed maszyną. VibeSSH go nie widzi — jeśli port nie odpowiada mimo poprawnych ustawień, sprawdź panel dostawcy.

## 4. Skonfiguruj

**Aplikacja → Pliki**. Edytor koloruje składnię, a pliki YAML dodatkowo sprawdza — przy błędzie składni zapis jest zablokowany, bo taki plik zatrzymałby serwer przy starcie.

Po zapisaniu konfiguracji zrestartuj aplikację: większość serwerów czyta pliki tylko przy starcie.

## 5. Włącz backupy

**Aplikacja → Backupy**. Ustaw harmonogram, zanim będzie potrzebny.

Dwie rzeczy warto wiedzieć od razu: harmonogram działa **tylko przy otwartym VibeSSH**, a backup leżący na tym samym węźle co aplikacja nie chroni przed utratą węzła. Cel zewnętrzny (S3) konfiguruje się w Ustawieniach.

## Co dalej

- **Kilka serwerów, które mają się widzieć** → rozdział Vibe Network.
- **Baza dla aplikacji** → rozdział Bazy danych.
- **Coś nie działa i nie wiadomo dlaczego** → rozdział Vibe AI, tryb Diagnoza.
- **Dostęp do czegoś, co nie ma być publiczne** → rozdział Tunele SSH.

## Kolejność, która oszczędza kłopotów

1. Test połączenia przed zapisaniem serwera.
2. Synchronizacja firewalla po każdej zmianie dostępu portu.
3. Restart aplikacji po każdej zmianie pliku konfiguracyjnego.
4. Backup przed każdą zmianą, której nie umiesz cofnąć.
