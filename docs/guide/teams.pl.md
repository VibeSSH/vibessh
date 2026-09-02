---
id: teams
title: Zespoły
section: getting-started
route: /teams
order: 85
---

Zespoły pozwalają dzielić dostęp do serwerów z innymi osobami. To jedyna część VibeSSH, która wymaga konta w chmurze — reszta aplikacji działa lokalnie i nie potrzebuje żadnego logowania.

## Gdzie to jest

Menu boczne → **Zespoły**. Pozycja pojawia się po zalogowaniu.

## Zespół

Tworzysz zespół, podając nazwę. Osoba, która go utworzyła, jest **właścicielem**.

## Zapraszanie

Zapraszasz kogoś, a on dostaje **kod zaproszenia**. Kod wkleja u siebie w polu „Masz kod zaproszenia?" i akceptuje albo odrzuca.

Zaproszenie jest kodem, a nie automatycznym dodaniem: dopóki druga strona go nie użyje, nic się nie dzieje.

## Role

Członkowie mają role określające, co mogą robić z serwerami dzielonymi w zespole. Role ustawia się przy członku, na stronie zespołu.

## Serwery zespołu

Serwer udostępniony zespołowi widzą jego członkowie zgodnie ze swoimi rolami. Poświadczenia SSH pozostają tam, gdzie były — udostępnienie serwera nie rozsyła nikomu Twojego klucza prywatnego ani hasła.

## Usuwanie

Usunięcie członka odbiera mu dostęp do zasobów zespołu. Usunięcie zespołu jest nieodwracalne i dotyczy wszystkich jego członków.

## Częste pomyłki

- **Nie widzę Zespołów w menu** — trzeba być zalogowanym. Cała reszta VibeSSH działa bez konta.
- **Zaprosiłem i nic się nie stało** — zaproszenie to kod, który druga osoba musi wkleić u siebie.
- **Członek nie widzi serwera** — sprawdź, czy serwer jest udostępniony zespołowi i czy rola tej osoby na to pozwala.
