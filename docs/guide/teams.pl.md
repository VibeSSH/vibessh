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

## Dodawanie osoby

Zespół → **Członkowie** → **Utwórz konto**. Podajesz e-mail, opcjonalnie nazwę i wybierasz rolę. Konto powstaje od razu w zespole, z nadaną rolą, a Ty dostajesz wygenerowane hasło do przekazania.

**Hasło pokazujemy raz.** Nic nie trzyma go w czytelnej formie i żaden ekran go nie odtworzy — przekaż je, zanim zamkniesz panel. Jeśli przepadnie, trzeba założyć konto ponownie.

Ta osoba loguje się tym hasłem i **musi je natychmiast zmienić**. Do tego czasu backend odrzuca każde inne żądanie z tego konta, więc nie jest to prośba, tylko reguła — hasło, które znasz Ty, nie jest jeszcze hasłem tej osoby. Zmiana kończy przy okazji wszystkie inne sesje tego konta.

Wcześniej działało to przez kody zaproszeń. Zostało usunięte: zaproszenie docierało wyłącznie do kogoś, kto sam wcześniej się zarejestrował, czyli odwrotnie niż w przypadku, dla którego istniało.

## Role

Członkowie mają role określające, co mogą robić z serwerami dzielonymi w zespole. Role ustawia się przy członku, na stronie zespołu.

## Uprawnienia do operacji — i czym one nie są

Poza uprawnieniami do samego zespołu (członkowie, role, audyt) role niosą też uprawnienia do operacji: tworzenie aplikacji, porty, pliki, backupy, firewall, terminal, instalacja oprogramowania, Vibe Network.

> **To są barierki, nie granica bezpieczeństwa.** Ukrywają i blokują akcje wewnątrz VibeSSH, dzięki czemu nowy członek nie kliknie czegoś przez pomyłkę. Nie zatrzymają kogoś, kto nie chce być zatrzymany.

Powód jest architektoniczny i warto go znać: te operacje **nie przechodzą przez backend**. Aplikacja desktopowa wykonuje je własnym połączeniem SSH, poświadczeniami tej osoby, z jej komputera. Backend trzyma wyłącznie metadane serwerów zespołu i nigdy nie trzyma poświadczeń. Kto ma dostęp SSH do węzła, ten zrobi to samo zwykłym `ssh`, bez VibeSSH.

Uprawnienia obowiązują tylko dla serwerów **udostępnionych zespołowi**, dopasowanych po adresie i porcie. Twoje własne serwery, których nie ma w żadnym zespole, nie podlegają niczemu.

Jeśli aplikacja jest wylogowana albo nie ma łączności z backendem, nic nie jest ograniczane. To celowe: awaria sieci nie może zamknąć Ci dostępu do własnych maszyn.

### Kiedy potrzebna jest prawdziwa granica

Załóż tej osobie **konto na węźle** z ograniczonym `sudo` i własnym kluczem SSH. Egzekwuje to Linux, a nie interfejs, więc obowiązuje też poza VibeSSH. Uprawnienia w aplikacji i konto na węźle dobrze się uzupełniają: pierwsze porządkuje codzienną pracę, drugie wyznacza granicę.

## Serwery zespołu

Serwer udostępniony zespołowi widzą jego członkowie zgodnie ze swoimi rolami. Poświadczenia SSH pozostają tam, gdzie były — udostępnienie serwera nie rozsyła nikomu Twojego klucza prywatnego ani hasła.

## Usuwanie

Usunięcie członka odbiera mu dostęp do zasobów zespołu. Usunięcie zespołu jest nieodwracalne i dotyczy wszystkich jego członków.

## Częste pomyłki

- **Nie widzę Zespołów w menu** — trzeba być zalogowanym. Cała reszta VibeSSH działa bez konta.
- **Zaprosiłem i nic się nie stało** — zaproszenie to kod, który druga osoba musi wkleić u siebie.
- **Członek nie widzi serwera** — sprawdź, czy serwer jest udostępniony zespołowi i czy rola tej osoby na to pozwala.
