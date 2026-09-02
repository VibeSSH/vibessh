---
id: vibe-network
title: Vibe Network
section: network
route: /vibe-network
order: 40
---

Vibe Network to prywatna sieć łącząca Twoje węzły tunelem WireGuard. Dzięki niej aplikacja na jednym serwerze może rozmawiać z bazą na drugim, nie wystawiając jej portu do internetu.

## Gdzie to jest

Menu boczne → **Vibe Network**. Trzy zakładki: **Node'y**, **Endpointy**, **Prywatny DNS**.

## Dołączanie węzła

**Dodaj node** wybiera serwer z listy i robi na nim całą robotę: instaluje `wireguard-tools`, jeśli ich nie ma, generuje parę kluczy, przydziela adres w sieci prywatnej i uzgadnia konfigurację ze wszystkimi pozostałymi węzłami.

**Klucz prywatny nigdy nie opuszcza węzła.** Jest tworzony na jego dysku i tylko klucz publiczny wraca do aplikacji.

Używany jest osobny interfejs (`wg-vibessh0`) i nietypowy port UDP, żeby nie wejść w drogę WireGuardowi, który mógł już być na tym serwerze.

## Co mówi karta węzła

| Wiersz | Znaczenie |
| --- | --- |
| Nazwa DNS | Nazwa, pod którą węzeł jest widoczny w sieci prywatnej. |
| Połączenie | Stan **tunelu**, nie SSH. Wartości niżej. |
| Opóźnienie | Czas odpowiedzi węzła. |
| Aplikacje | Ile aplikacji na nim stoi. |
| Endpointy | Ile portów wystawia. |
| Ostatni handshake | Kiedy WireGuard ostatnio uzgodnił klucze z peerem. |

Plakietka **Online / Offline** u góry mówi, czy węzeł odpowiedział na SSH. To osobne pytanie od stanu tunelu i dlatego jest osobnym wskaźnikiem.

### Stany połączenia

- **Aktywne** — tunel działa i handshake jest świeży.
- **Nie zestawione** — tunel jest podniesiony, ale handshake nie nastąpił ani razu.
- **Bez ruchu** — handshake był, ale dawno.
- **Brak tunelu** — interfejsu nie ma na węźle. Nie dołączył albo nie został uzgodniony po dołączeniu.
- **Nieznane** — nie udało się odczytać stanu. Powód pojawia się w wierszu **Stan tunelu** poniżej.
- **Node nie odpowiada** — nie dało się do niego połączyć, więc nic nie zostało sprawdzone.

> Konfiguracja ustawia `PersistentKeepalive = 25`, więc **działający tunel robi handshake w pół minuty od podniesienia, nawet jeśli nikt nic nie przesyła**. „Nie zestawione" utrzymujące się dłużej to realny problem, a nie brak ruchu.

Wiersz **Nieznane peery** pojawia się, gdy węzeł raportuje peera, którego klucza nie zna żaden node w tej sieci. Tak wygląda węzeł przekluczowany poza aplikacją.

## Synchronizacja

**Synchronizuj Vibe Network** doprowadza wszystko do zadeklarowanego stanu: uzgadnia peery WireGuarda na każdym węźle, nakłada reguły firewalla i rozsyła wpisy prywatnego DNS. Każdy węzeł i każdy z tych trzech kroków jest wykonywany niezależnie — jeden nieosiągalny węzeł nie blokuje pozostałych.

Wynik pokazuje się listą pod przyciskiem, z powodem przy każdym niepowodzeniu.

## Prywatny DNS

Nadaje węzłom i aplikacjom nazwy w rodzaju `vps.vibe`, żeby konfiguracja mogła wskazywać na nazwę, a nie na adres, który zmieni się przy przenosinach.

## Endpointy

Widok wszystkich portów aplikacji zebranych per węzeł. To ten sam model danych co zakładka Porty w aplikacji, tylko oglądany z drugiej strony — nie ma tu osobnych bytów do konfigurowania.

## Częste pomyłki

- **Sync mówi „udane", a połączenie „Nie zestawione"** — synchronizacja rozsyła konfigurację, ale to nie ona robi handshake. Sprawdź, czy port UDP WireGuarda jest przepuszczony w firewallu dostawcy VPS-a.
- **Aplikacje nadal się nie widzą** — sam tunel nie daje im dostępu do siebie. Połączenia między aplikacjami nadaje się osobno, na zakładce Porty aplikacji.
- **Usunąłem węzeł z sieci i klucz zniknął** — nie zniknął. Opuszczenie sieci zdejmuje interfejs i konfigurację, ale para kluczy zostaje, więc ponowne dołączenie nie zmienia tożsamości węzła.
