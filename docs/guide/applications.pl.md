---
id: applications
title: Aplikacje
section: applications
route: /applications
order: 10
---

Aplikacja to jedna rzecz, która działa na węźle: serwer Minecrafta, proxy Velocity, baza, bot. VibeSSH prowadzi jej cały cykl życia — utworzenie, start, konfigurację, pliki, backupy — i robi to zdalnie po SSH, bez agenta na węźle, chyba że sam go zainstalujesz.

## Blueprint

Aplikację tworzy się z blueprintu, czyli gotowego przepisu: jaki obraz, jakie porty, jakie pliki konfiguracyjne, jakie zmienne. Blueprint jest punktem wyjścia, a nie klatką — po utworzeniu każdą z tych rzeczy możesz zmienić.

Jedyne, czego blueprint pilnuje na stałe, to porty oznaczone jako **Wymagane**: można je edytować, ale nie usunąć.

## Środowisko uruchomieniowe

| Typ | Co to znaczy |
| --- | --- |
| Docker | Aplikacja żyje w kontenerze na węźle. Domyślne i najlepiej wspierane. |
| Systemd | Usługa systemowa na węźle. |
| Proces lokalny | Proces na tym komputerze, nie na węźle. |

Reszta poradnika opisuje przede wszystkim Dockera, bo to on stoi za pojęciami takimi jak odtworzenie kontenera czy publikowane porty.

## Operacje

- **Uruchom** — bez pytania o potwierdzenie. Start niczym nie ryzykuje i cofa się jednym kliknięciem.
- **Zatrzymaj** — z potwierdzeniem, bo zrywa połączenia wszystkim, którzy są w środku.
- **Uruchom ponownie** — zatrzymanie i start tego samego kontenera.
- **Wymuś zakończenie** — zabija proces bez czekania. Serwer nie zapisze świata. Ostatnia deska ratunku, nie codzienne narzędzie.
- **Odtwórz kontener** — usuwa kontener i tworzy go od nowa z aktualnej konfiguracji. **Nie rusza danych** — pliki aplikacji leżą poza kontenerem.

### Kiedy potrzebne jest odtworzenie

Docker zapisuje część konfiguracji w kontenerze w chwili jego tworzenia, a nie odczytuje jej przy każdym starcie: publikowane porty, limity zasobów, zmienne środowiskowe, obraz. Zwykły restart użyłby tego samego, nieaktualnego kontenera.

Dlatego zmiana tych rzeczy na **działającej** aplikacji odtwarza kontener automatycznie. Aplikacja zatrzymana zostaje zatrzymana i dostanie nową konfigurację przy najbliższym starcie.

## Konsola

Konsola na zakładce Przegląd pokazuje wyjście aplikacji i pozwala wysyłać do niej polecenia.

Dla aplikacji dockerowej na węźle po SSH to **prawdziwy strumień** (`docker logs -f`) — linie pojawiają się w momencie, w którym proces je wypisuje. Dla pozostałych środowisk konsola odpytuje logi co dwie sekundy. Znacznik przy nagłówku mówi, który tryb jest aktywny: *Na żywo* albo *Odpytywanie*.

Linie są kolorowane po poziomie ważności — ostrzeżenia na pomarańczowo, błędy na czerwono.

Jeśli strumień się urwie (np. węzeł odświeżył połączenie), konsola próbuje wznowić kilka razy z rosnącym odstępem, a dopiero potem schodzi na odpytywanie.

## Zakładki

- **Przegląd** — status, konsola, wykresy CPU i RAM, podstawowe fakty.
- **Pliki** — przeglądarka i edytor plików aplikacji.
- **Logi** — pełniejszy odczyt logów niż okno konsoli.
- **Porty** — co jest wystawione i dla kogo.
- **Bazy danych** — bazy przypisane do tej aplikacji.
- **Backupy** — kopie katalogu roboczego.
- **Ustawienia** — konfiguracja, obraz, limity zasobów, zmienne środowiskowe, health check.

## Migracja na inny węzeł

Przenosi aplikację razem z danymi na inny węzeł. Operacja jest długa i wymaga, żeby oba węzły były osiągalne. To nie jest sposób na klonowanie — źródło przestaje być właścicielem aplikacji.

## Częste pomyłki

- **Zmieniłem konfigurację i nic się nie stało** — jeśli aplikacja była zatrzymana, zmiana czeka na start. Jeśli działała, kontener został odtworzony.
- **Konsola nic nie pokazuje** — sprawdź, czy aplikacja działa. Zatrzymany kontener nie produkuje wyjścia, a poprzednie linie znikają po odtworzeniu kontenera.
- **Wymuś zakończenie zamiast Zatrzymaj** — serwer gry nie zdąży zapisać świata. Zatrzymuj normalnie, chyba że proces przestał odpowiadać.
