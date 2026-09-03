---
id: applications
title: Aplikacje
section: applications
route: /applications
order: 20
---

Aplikacja to jedna rzecz działająca na Node: serwer Minecraft, proxy, bot, baza.

![Przegląd aplikacji: konsola z pokolorowanymi logami i zużycie zasobów obok](images/app-overview.png)

## Jak utworzyć aplikację

1. Otwórz **Aplikacje**.
2. Kliknij **Utwórz aplikację**.
3. **Krok 1 — Lokalizacja i podstawy**: w polu **Gdzie ma działać?** wybierz Node. Wpisz **Nazwę** i **Katalog roboczy**, np. `/srv/paper`.
4. **Krok 2 — Obraz i runtime**: wybierz **Obraz** (np. Paper) i **Runtime**.
5. **Krok 3 — Konfiguracja**: wybierz wersję. Dla serwerów Javy wybierz też instalację Javy.
6. **Krok 4 — Zmienne środowiskowe**: możesz pominąć. Zmienne dodasz później.
7. **Krok 5 — Podsumowanie**: sprawdź dane i zatwierdź.
8. Na stronie aplikacji kliknij **Uruchom**.

Kreator nie ustawia RAM-u ani portów. Robisz to po utworzeniu, w zakładkach **Ustawienia** i **Porty**.

## Sterowanie aplikacją

Przyciski na górze strony aplikacji:

| Przycisk | Co robi |
| --- | --- |
| **Uruchom** | Startuje aplikację. |
| **Zatrzymaj** | Zatrzymuje w sposób kontrolowany. |
| **Uruchom ponownie** | Zatrzymuje i uruchamia. |
| **Wymuś zakończenie** | Ubija natychmiast. Serwer nie zapisze świata. |
| **Odtwórz kontener** | Buduje kontener od nowa. Nie kasuje plików aplikacji. |
| **Migruj do innego Node'a** | Przenosi aplikację razem z danymi. |

## Konsola

Konsola jest w zakładce **Przegląd**.

1. Wpisz komendę w polu na dole.
2. Wciśnij Enter albo kliknij **Wyślij**.

Ostrzeżenia są pomarańczowe, błędy czerwone.

## Ustawienie RAM-u i CPU

1. Wejdź w zakładkę **Ustawienia**.
2. Znajdź kartę **Limity zasobów** i kliknij **Edytuj**.
3. **Pamięć** — wpisz w MB, np. `2048`. Puste pole oznacza brak limitu.
4. **CPU** — wpisz liczbę rdzeni, np. `1.5`.
5. Zapisz.

## Zmienne środowiskowe

1. Wejdź w zakładkę **Ustawienia**.
2. Kliknij **Dodaj zmienną**.
3. Wpisz **Klucz** i **Wartość**.
4. Dla haseł i kluczy API zaznacz **Sekret**.
5. Zapisz.

## Jak sprawdzić, czy działa

- Status aplikacji to **Działa**.
- W konsoli pojawiają się nowe linie.
- Karta **Zużycie zasobów** pokazuje CPU i RAM.

## Najczęstsze problemy

- **Zmieniłem konfigurację i nic się nie stało** — jeśli aplikacja była zatrzymana, zmiana zadziała przy starcie.
- **Konsola jest pusta** — aplikacja nie działa albo kontener został właśnie odtworzony.
- **Aplikacja nie startuje** — otwórz zakładkę **Logi** i przeczytaj ostatnie linie.

## Więcej informacji

Docker zapisuje porty, limity, zmienne i obraz w kontenerze w chwili jego tworzenia. Dlatego zmiana któregokolwiek z nich na działającej aplikacji odtwarza kontener automatycznie. Dane aplikacji leżą w katalogu roboczym poza kontenerem i nie są przy tym ruszane.
