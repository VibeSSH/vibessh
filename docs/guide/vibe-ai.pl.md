---
id: vibe-ai
title: Vibe AI
section: ai
route: /vibe-ai
order: 80
---

Vibe AI odpowiada na pytania o VibeSSH i pomaga zdiagnozować węzeł albo aplikację, która sprawia problemy. Czyta ten sam poradnik, który masz otwarty, więc odpowiedzi opisują tę wersję aplikacji, a nie ogólną wiedzę o Dockerze z internetu.

## Czego asystent nie robi

To jest równie ważne jak to, co robi:

- **Nie ma dostępu do terminala** i nie wykonuje poleceń. Może powiedzieć, co wpisać — wpisujesz Ty.
- **Nie zmienia niczego** w konfiguracji, plikach ani stanie aplikacji.
- **Nie działa w tle.** Nie ma tu autonomicznego agenta obserwującego Twoje węzły.

## Dwa tryby

**Pytanie** — odpowiada na pytania o VibeSSH. **Nic nie jest odczytywane z Twoich węzłów.** Do tego trybu nadają się pytania w rodzaju „co robi odtworzenie kontenera" albo „czym różni się dostęp Vibe Network od publicznego".

**Diagnoza** — dodatkowo wysyła migawkę wybranego węzła albo aplikacji: status, ostatnie logi, konfigurację. Tego trybu używasz, gdy coś nie działa i nie wiadomo dlaczego.

## Co dokładnie zostanie wysłane

Panel **Co zostanie wysłane** pokazuje dokładną treść, która pojedzie razem z Twoją wiadomością. Nie streszczenie i nie opis — to samo, co zobaczy model.

Przed wysłaniem migawka jest czyszczona: **hasła, tokeny, klucze prywatne, sekretne zmienne środowiskowe i hasła w connection stringach są usuwane.** Dotyczy to również wartości podanych w wierszu poleceń kontenera i nagłówków autoryzacji w logach.

Zaglądaj do tego panelu, jeśli masz wątpliwości. Jest po to, żeby nie trzeba było ufać opisowi.

## Konfiguracja

**Ustawienia → AI**. Model współdzielony (Qwen) działa bez własnego klucza i ma dzienny limit pytań na użytkownika — licznik zużycia jest w tym samym miejscu.

Chcąc użyć innego modelu, podajesz własne dane dostępowe: adres bazowy, nazwę modelu i klucz API. **Klucz trafia do magazynu poświadczeń systemu**, nigdy do pliku konfiguracyjnego, i nie wraca do interfejsu po zapisaniu. **Testuj połączenie** sprawdza, czy ustawienia działają, zanim zadasz pierwsze pytanie.

## Zapytaj Vibe AI z miejsca błędu

Tam, gdzie aplikacja zgłasza błąd, pojawia się przycisk **Zapytaj Vibe AI**. Otwiera asystenta z już wybranym kontekstem i gotowym pytaniem, więc nie trzeba przepisywać komunikatu ręcznie.

## Jak czytać odpowiedzi

Asystent ma wskazać **jedną prawdopodobną przyczynę** i konkretny następny krok, a nie wyliczać wszystkiego, co teoretycznie mogło zawieść. Jeśli odpowiedź rozjeżdża się w listę możliwości, zwykle znaczy to, że w kontekście było za mało — spróbuj w trybie Diagnoza z wybraną właściwą aplikacją.

Odpowiedź może być niepełna, jeśli zatrzymasz ją przyciskiem **Zatrzymaj** — jest to wtedy wyraźnie oznaczone.

## Częste pomyłki

- **„Asystent nie jest jeszcze skonfigurowany"** — brak konfiguracji w Ustawieniach albo wyczerpany dzienny limit modelu współdzielonego.
- **Odpowiedź nie zna mojej aplikacji** — tryb Pytanie nic nie odczytuje z węzłów. Przełącz na Diagnozę i wybierz aplikację.
- **Poprosiłem, żeby coś naprawił** — asystent nie wykona żadnej operacji. Opisze, co zrobić.
