---
id: account-security
title: Weryfikacja dwuetapowa
section: getting-started
route: /settings
order: 155
---

Weryfikacja dwuetapowa (2FA) sprawia, że do zalogowania się na konto VibeSSH potrzebne jest hasło i kod z telefonu. Ktoś, kto pozna Twoje hasło, nie wejdzie bez telefonu.

## Czego potrzebujesz

Aplikacji uwierzytelniającej na telefonie, np. **Google Authenticator**, **Aegis** albo **1Password**.

## Jak włączyć

1. Zaloguj się na konto VibeSSH.
2. Wejdź w **Ustawienia** → karta **Konto i bezpieczeństwo**.
3. Kliknij **Włącz**, potem **Dalej**.
4. Zeskanuj kod QR aplikacją na telefonie. Jeśli nie możesz, wpisz w niej klucz pokazany pod kodem.
5. Wpisz 6-cyfrowy kod, który pokazuje aplikacja, i kliknij **Włącz**.
6. Zapisz **kody zapasowe** w bezpiecznym miejscu (np. menedżerze haseł). Kliknij **Kopiuj kody**, zaznacz **Zapisałem kody zapasowe** i kliknij **Gotowe**.

Kody zapasowe widać tylko raz. Bez nich i bez telefonu nie wejdziesz na konto.

## Jak się logować

1. Wpisz e-mail i hasło jak zwykle.
2. Gdy pojawi się pole **Kod z aplikacji**, wpisz aktualny kod z telefonu.

Nie masz telefonu? Kliknij **Nie mam telefonu - użyj kodu zapasowego** i wpisz jeden z zapisanych kodów. Każdy kod zapasowy działa tylko raz.

## Jak wyłączyć

1. **Ustawienia** → **Konto i bezpieczeństwo** → **Wyłącz**.
2. Wpisz hasło i kod z aplikacji (albo kod zapasowy).
3. Kliknij **Wyłącz**.

## Najczęstsze problemy

- **„Ten kod jest nieprawidłowy”** - najczęściej zegar w telefonie się spieszy albo spóźnia. Włącz automatyczną godzinę w ustawieniach telefonu. Ten sam kod działa też tylko raz, więc po użyciu poczekaj na następny.
- **Zgubiłem telefon** - zaloguj się kodem zapasowym, wyłącz weryfikację dwuetapową i włącz ją ponownie na nowym telefonie.
- **„Weryfikacja dwuetapowa nie jest dostępna na tym serwerze kont”** - serwer kont nie ma skonfigurowanego klucza szyfrującego. Dotyczy to tylko własnego serwera kont, a nie api.vibessh.dev.
- **Aplikacja na telefonie nie pyta o kod** - aplikacja mobilna VibeSSH nie obsługuje jeszcze kodów. Do czasu aktualizacji loguj się z komputera.

## Więcej informacji

Klucz, z którego telefon wylicza kody, jest przechowywany na serwerze kont w postaci zaszyfrowanej. Kod QR jest rysowany na Twoim komputerze, więc klucz nie trafia do żadnej zewnętrznej usługi.
