---
id: application-files
title: Pliki aplikacji
section: files
route: /applications
order: 20
---

Przeglądarka plików pokazuje katalog roboczy aplikacji — i tylko jego. Nie jest to menedżer plików całego serwera; z tej zakładki nie da się wyjść poza katalog aplikacji, nawet wpisując ścieżkę ręcznie.

## Gdzie to jest

Aplikacja → zakładka **Pliki**.

## Konto, na którym to działa

Jeśli aplikacja ma włączone dedykowane konto, VibeSSH zakłada na węźle osobne konto systemowe (`vibessh-app-…`) i wszystkie operacje na plikach wykonuje jako ono. To nie jest kosmetyka: bez tego każda aplikacja na węźle mogłaby czytać pliki każdej innej.

Konto i skrypt pomocniczy są zakładane przy pierwszej operacji na plikach, więc pierwsze wejście w zakładkę może być odrobinę wolniejsze niż kolejne.

![Przeglądarka plików aplikacji z katalogami i plikami konfiguracyjnymi](images/application-files.png)

## Nawigacja

Klik w katalog wchodzi do środka, okruszki nad listą prowadzą z powrotem. Katalog raz odwiedzony jest zapamiętany, więc cofanie się po drzewie jest natychmiastowe — dopiero po chwili VibeSSH pyta węzeł, czy coś się zmieniło.

Lista pokazuje najwyżej 200 pozycji. Pole **Filtruj** zawęża do tego, czego szukasz — katalog z tysiącami plików jest po to, żeby go filtrować, a nie przewijać.

## Edytor

Klik w plik otwiera go w edytorze z kolorowaniem składni. Rozpoznawane są między innymi `.yml`, `.json`, `.properties`, `.toml`, `.sh`, `Dockerfile` i pliki jednostek systemd.

- **Szukaj** (ikona lupy albo `Ctrl+F`) — wyszukiwarka w pliku, z podświetlaniem trafień.
- **Ctrl+S** — zapis, tak samo jak przycisk.
- **Historia** — poprzednie wersje pliku zapisane przez ten edytor.
- **Kopia zapasowa przed zapisem** — zaznaczone domyślnie. Przed nadpisaniem pliku jego obecna treść trafia do historii. Wyłączaj świadomie.

Pliki powyżej 1 MB nie otwierają się w edytorze. Limit egzekwuje backend, nie tylko interfejs.

### Walidacja YAML

Pliki `.yml` i `.yaml` są sprawdzane składniowo w trakcie pisania. Linia z błędem dostaje tło, belkę przy lewej krawędzi i kropkę w rynience.

**Przy błędzie składni zapis jest zablokowany.** To celowe: plik konfiguracyjny, który się nie parsuje, powoduje, że serwer nie wstaje, a awaria pojawia się minutę później, w zupełnie innym miejscu, bez śladu przyczyny.

Blokują wyłącznie **błędy**. Ostrzeżenia nie — ostrzeżenie to parser mówiący „nietypowe", a odmowa zapisu byłaby edytorem przegłosowującym Ciebie.

Duplikat klucza też blokuje. To ten przypadek, w którym drugi po cichu wygrywa, więc ustawienie ma inną wartość, niż widać w pliku.

## Operacje na plikach

Prawy przycisk myszy na pozycji: zmiana nazwy, przeniesienie, kopiowanie, uprawnienia, usunięcie, a dla archiwów `.zip` — rozpakowanie.

Wysyłanie i pobieranie idzie przez kolejkę transferów widoczną na dole. Transfer, który padnie, można ponowić bez wybierania pliku od nowa.

## Częste pomyłki

- **Nie widzę pliku, który wgrałem przez FTP** — odśwież listę. Zapamiętany katalog pokazuje ostatni odczyt, dopóki VibeSSH nie zapyta ponownie.
- **Zapis nie działa i nie wiem czemu** — jeśli to YAML, spójrz na czerwony pasek nad edytorem. Podaje numer linii i powód.
- **Zmieniłem plik, a serwer działa po staremu** — większość serwerów czyta konfigurację przy starcie. Po zapisie trzeba zrestartować aplikację.
