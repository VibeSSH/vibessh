---
id: vibe-ai
title: Vibe AI
section: ai
route: /vibe-ai
order: 130
---

Vibe AI pomaga analizować błędy i konfigurację.

## Jak skonfigurować

1. Otwórz **Ustawienia**.
2. Znajdź kartę **Vibe AI**.
3. Zaznacz **Włącz asystenta Vibe AI**.
4. **Dostawca** — wybierz z listy.
5. **Adres bazowy** — adres API dostawcy.
6. **Model** — nazwa modelu.
7. **Klucz API** — wklej swój klucz.
8. Kliknij test połączenia.
9. Zapisz.

Model współdzielony działa bez własnego klucza i ma dzienny limit pytań.

## Jak zadać pytanie

1. Otwórz **Vibe AI** w menu bocznym.
2. Wybierz tryb:
   - **Pytanie** — odpowiedzi o VibeSSH. Nic nie jest odczytywane z Twoich serwerów.
   - **Diagnoza** — dodatkowo wysyła dane wybranego serwera lub aplikacji.
3. W trybie **Diagnoza** wybierz serwer albo aplikację.
4. Wpisz pytanie i wciśnij Enter.

## Jak zapytać o błąd aplikacji

1. Otwórz aplikację, która zgłasza błąd.
2. Kliknij **Zapytaj Vibe AI**.
3. Asystent otworzy się z gotowym pytaniem i wybranym kontekstem.

## Co zostanie wysłane

Panel **Co zostanie wysłane** pokazuje dokładną treść, która pojedzie z Twoją wiadomością. Hasła, tokeny, klucze prywatne i sekretne zmienne środowiskowe są wcześniej usuwane.

## Jak sprawdzić, czy działa

- Test połączenia kończy się powodzeniem.
- Po wysłaniu pytania pojawia się odpowiedź.
- W trybie **Diagnoza** panel **Co zostanie wysłane** pokazuje dane wybranej aplikacji.

## Najczęstsze problemy

- **Asystent nie jest skonfigurowany** — brak ustawień w **Ustawieniach** albo wyczerpany dzienny limit modelu współdzielonego.
- **Odpowiedź nie zna mojej aplikacji** — tryb **Pytanie** nic nie odczytuje z serwerów. Przełącz na **Diagnoza**.
- **Poprosiłem, żeby coś naprawił, i nic się nie stało** — asystent nie wykonuje żadnych operacji. Opisze, co zrobić.

## Więcej informacji

Klucz API trafia do magazynu poświadczeń systemu i nie wraca do interfejsu po zapisaniu. Asystent nie ma dostępu do terminala i nie działa w tle.
