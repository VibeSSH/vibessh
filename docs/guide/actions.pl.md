---
id: actions
title: Akcje
section: nodes
route: /actions
order: 68
---

Akcje pokazują to, co na węźle działa **poza** VibeSSH: usługi systemd i kontenery Dockera, których nie utworzyłeś tutaj jako aplikacji.

Aplikacjami zarządza się w module Aplikacje. Ten ekran jest do reszty maszyny — usług systemowych, cudzych kontenerów, rzeczy postawionych ręcznie zanim VibeSSH się pojawiło.

## Gdzie to jest

Menu boczne → **Akcje**, po wybraniu serwera.

## Usługi systemd

Lista jednostek z dwoma niezależnymi stanami, których nie należy mylić:

- **Aktywna / Nieaktywna** — czy usługa działa **w tej chwili**.
- **Włączona / Wyłączona** — czy wstanie **po restarcie systemu**.

To są osobne rzeczy. Usługa może działać i nie być włączona (po restarcie serwera zniknie), albo być włączona i nie działać (padła i czeka na następny start).

Dostępne operacje to uruchom, zatrzymaj, zrestartuj oraz włącz i wyłącz przy starcie systemu. Filtr po nazwie zawęża listę — typowy serwer ma ich ponad sto.

## Kontenery Docker

Kontenery na węźle, z ich stanem i podglądem logów. Jeśli lista jest pusta, na węźle albo nie ma kontenerów, albo nie ma Dockera.

## Uwaga

Ten ekran działa na prawdziwym systemie węzła. Zatrzymanie usługi, od której zależy coś innego, zatrzyma także to coś — i nie ma tu potwierdzenia z opisem konsekwencji, bo VibeSSH nie wie, do czego dana jednostka służy w Twojej konfiguracji.

Szczególnie ostrożnie z usługami sieci i SSH: zatrzymanie `ssh` odcina VibeSSH od węzła i jedyną drogą powrotu jest konsola u dostawcy VPS-a.

## Częste pomyłki

- **Zatrzymałem kontener aplikacji tutaj** — możliwe, ale zarządzanie aplikacją jest w jej własnym widoku, razem z konsolą, portami i backupami.
- **Usługa działa, a po restarcie serwera jej nie ma** — była aktywna, ale nie włączona. To dwie różne rzeczy.
- **Nie widzę żadnych kontenerów** — sprawdź, czy Docker jest zainstalowany i czy konto ma do niego dostęp.
