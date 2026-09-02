---
id: terminal
title: Terminal
section: nodes
route: /terminal
order: 60
---

Terminal to zwykła powłoka na węźle — to samo, co dostałbyś, łącząc się przez `ssh`, tylko w oknie aplikacji i z zakładkami.

## Gdzie to jest

Menu boczne → **Terminal**, po wybraniu serwera. Skrót do tego samego jest na Panelu, na zakładce Terminal wybranego węzła.

## Karty

**Nowy terminal** otwiera kolejną kartę. Każda karta to osobna sesja SSH z własną historią i własnym katalogiem roboczym — zamknięcie jednej nie rusza pozostałych.

Karty nie przeżywają zamknięcia aplikacji. To sesje interaktywne, nie `screen` ani `tmux`; jeśli potrzebujesz procesu przeżywającego rozłączenie, uruchom go w `tmux` na węźle albo zrób z niego aplikację.

## Wyszukiwanie

Pole **Szukaj w terminalu** przeszukuje bufor przewijania bieżącej karty, z przejściem do poprzedniego i następnego trafienia.

## Zakończenie sesji

Gdy sesja się skończy — bo wpisałeś `exit`, bo serwer ją zamknął, bo padło połączenie — karta zostaje z komunikatem i podanym powodem, jeśli jest znany. **Karta nie łączy się ponownie sama.** Zamknij ją i otwórz nową; automatyczne wznawianie po cichu wróciłoby do innego stanu, niż zostawiłeś.

## Tylko SSH

Terminal działa dla węzłów w trybie SSH. Węzeł w trybie Agent nie udostępnia z tej aplikacji powłoki.

## Uwaga o Vibe AI

Asystent **nie ma dostępu do terminala** i nie wykonuje poleceń. Może opisać, co zrobić, ale wpisujesz to sam. To jest granica projektowa, nie brak funkcji.

## Częste pomyłki

- **Uruchomiłem serwer w terminalu i zniknął po zamknięciu karty** — proces wystartowany w sesji interaktywnej ginie razem z nią. Do tego są aplikacje.
- **Terminala nie ma w menu** — nie wybrano serwera, albo węzeł jest w trybie Agent.
- **Kopiowanie i wklejanie** — działa jak w terminalu, nie jak w edytorze tekstu; zaznaczenie kopiuje, prawy przycisk wkleja.
