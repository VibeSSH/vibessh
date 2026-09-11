---
id: cloud-backend
title: Backend kont
section: getting-started
route: /settings
order: 155
---

Konta, zespoły i współdzielone serwery potrzebują backendu — osobnego programu, który trzyma użytkowników i uprawnienia. Cała reszta VibeSSH działa bez niego: Node'y, aplikacje, pliki, terminal, firewall.

**VibeSSH nie hostuje tego za Ciebie.** Backend uruchamiasz sam, a w aplikacji wskazujesz jego adres. Dopóki tego nie zrobisz, rejestracja i logowanie kończą się błędem „couldn't reach the VibeSSH cloud backend" z adresem `http://localhost:8787` — to adres domyślny, czyli serwer na Twoim własnym komputerze, którego tam nie ma.

## Kiedy w ogóle tego potrzebujesz

Potrzebujesz, jeśli chcesz **konta i zespoły** — kilka osób pracujących na tych samych serwerach, z podziałem uprawnień.

Nie potrzebujesz, jeśli używasz VibeSSH sam. Dodawanie Node'ów, stawianie aplikacji, pliki i terminal działają bez żadnego backendu i bez logowania.

## Jak postawić własny backend

Backend jest w katalogu `backend/` w źródłach VibeSSH. Najprościej uruchomić go Dockerem — na tym samym serwerze, na którym trzymasz Node'y, albo na dowolnym innym.

1. Skopiuj katalog `backend/` na serwer.
2. Skopiuj `.env.example` do `.env` i uzupełnij **dwie rzeczy**, bez których nic nie wstanie:
   - **POSTGRES_PASSWORD** — hasło do bazy, dowolne długie.
   - **JWT_SECRET** — sekret podpisujący tokeny logowania. Wygeneruj prawdziwy, na przykład `openssl rand -base64 48`. Zostawienie tam tekstu z przykładu oznacza sekret, który zna każdy, kto widział te źródła.
3. W tym katalogu uruchom:

```
docker compose up -d
```

4. Sprawdź, czy odpowiada. Z tego samego serwera:

```
curl http://localhost:8787/health
```

5. Otwórz port `8787`, jeśli aplikacja ma się łączyć z zewnątrz. Zajrzyj do poradnika **Firewall**.

Backend trzyma dane w PostgreSQL-u, w wolumenie Dockera `vibessh-postgres-data`. Kopię zapasową robisz przez `docker compose exec postgres pg_dump -U vibessh_app vibessh > kopia.sql` — kopiowanie katalogu z działającą bazą pod spodem daje plik, którego może się nie dać odtworzyć.

## Jak wskazać go aplikacji

1. Otwórz **Ustawienia**.
2. Znajdź kartę **Backend kont**.
3. W polu **Adres backendu** wpisz pełny adres z protokołem, np. `https://konta.mojadomena.pl` albo `http://94.130.201.103:8787`.
4. Kliknij **Zapisz**.

Dopóki w polu stoi adres domyślny, karta pokazuje czerwone ostrzeżenie. Zniknie, gdy wpiszesz własny.

Adres zapisuje się na tym komputerze, więc **każda osoba w zespole musi wpisać go u siebie** — inaczej zobaczy ten sam błąd.

## Bezpieczeństwo

Jeśli backend ma być dostępny z internetu, postaw go za HTTPS. Przez `http://` hasła i tokeny sesji lecą po sieci otwartym tekstem — to jest ta sama sieć, przez którą ktoś zarządza serwerami z uprawnieniami roota.

Najprościej: postaw przed nim nginx albo Caddy z certyfikatem Let's Encrypt i wskaż w VibeSSH adres `https://`. Do użytku w sieci lokalnej albo przez tunel SSH `http://` wystarczy.

## Częste problemy

**„couldn't reach the VibeSSH cloud backend ... this is the development default".** Adres nie został jeszcze zmieniony. Patrz sekcja wyżej.

**Adres wpisany, a i tak nie odpowiada.** Sprawdź po kolei: czy backend działa (`docker compose ps`), czy port jest otwarty w firewallu Node'a, i czy adres ma protokół — samo `mojadomena.pl` bez `https://` nie zadziała.

**Działa u Ciebie, nie działa u kolegi z zespołu.** Adres jest ustawieniem lokalnym każdej instalacji. Musi go wpisać u siebie.
