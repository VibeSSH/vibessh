---
id: teams
title: Zespoły
section: getting-started
route: /teams
order: 150
---

Zespoły pozwalają dać innym osobom dostęp do VibeSSH. Wymagają konta w chmurze; reszta aplikacji działa lokalnie.

## Jak utworzyć zespół

1. Otwórz **Zespoły**.
2. Wpisz nazwę zespołu.
3. Kliknij **Utwórz**.

## Jak dodać osobę

1. Otwórz zespół → zakładka **Członkowie**.
2. Zjedź do karty **Utwórz konto**.
3. **E-mail** — adres tej osoby.
4. **Nazwa** — opcjonalnie.
5. **Rola** — wybierz rolę z listy.
6. Kliknij **Utwórz konto**.
7. Skopiuj wyświetlone hasło i przekaż je tej osobie.

Hasło pokazujemy raz. Jeśli je zgubisz, załóż konto ponownie.

Ta osoba loguje się tym hasłem i musi je od razu zmienić. Do tego czasu jej konto nie może zrobić nic innego.

## Jak utworzyć rolę

1. Otwórz zespół → zakładka **Role**.
2. Kliknij **Utwórz rolę**.
3. Wpisz **Nazwę** i opcjonalnie **Opis**.
4. Zaznacz uprawnienia w grupach.
5. Zapisz.

## Jak udostępnić serwer zespołowi

1. Otwórz zespół → zakładka **Serwery**.
2. Dodaj serwer, podając jego dane.

Uprawnienia do operacji działają tylko dla serwerów udostępnionych zespołowi.

## Jak sprawdzić, czy działa

- Osoba jest na liście **Członkowie**.
- Rola jest widoczna przy tej osobie.
- Po zalogowaniu widzi tylko te przyciski, na które pozwala rola.

## Najczęstsze problemy

- **Nie widzę Zespołów w menu** — trzeba być zalogowanym.
- **Konto z tym adresem już istnieje** — ta osoba ma już konto. Dodaj ją jako członka zamiast tworzyć nowe.
- **Nowy członek i tak może wszystko** — serwer nie jest udostępniony zespołowi, albo osoba korzysta z tego samego komputera co Ty i widzi Twoje lokalne serwery.

## Więcej informacji

Uprawnienia do operacji na serwerach (aplikacje, porty, pliki, firewall, terminal) ukrywają i blokują akcje w VibeSSH. Nie zatrzymają kogoś, kto ma dostęp SSH do serwera poza aplikacją. Prawdziwą granicę daje osobne konto na serwerze z ograniczonym `sudo`.
