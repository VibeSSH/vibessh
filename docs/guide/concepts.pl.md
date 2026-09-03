---
id: concepts
title: Node i aplikacja
section: getting-started
route: /
order: 0
---

Cała reszta poradnika stoi na dwóch pojęciach. Warto poświęcić im pięć minut, bo mylenie ich ze sobą jest źródłem większości nieporozumień: „usunąłem kontener i straciłem świat", „zmieniłem konfigurację i nic się nie stało", „po co mi Vibe Network, skoro serwery są moje".

## Node

**Node to maszyna, którą VibeSSH obsługuje.** VPS, serwer w szafie, komputer w piwnicy. Jedna maszyna, jeden Node.

VibeSSH nie jest na niej zainstalowany. Łączy się z nią po SSH — dokładnie tak, jak zrobiłbyś to sam z terminala, Twoim kontem i Twoim kluczem. Wszystko, co widzisz w aplikacji, jest wynikiem poleceń wykonanych na tej maszynie w Twoim imieniu.

To ma trzy konsekwencje, które wracają w całym poradniku:

- **Uprawnienia na węźle to uprawnienia Twojego konta SSH.** Jeśli konto nie ma `sudo`, VibeSSH też go nie ma.
- **Co VibeSSH wie o węźle, wie z zapytania.** „Offline" znaczy „nie udało mi się połączyć", a nie „maszyna nie żyje".
- **Zmiany są prawdziwe.** Zatrzymanie usługi w zakładce Akcje zatrzymuje ją naprawdę, ze wszystkim, co od niej zależy.

Alternatywny tryb — **Vibe Agent** — instaluje na maszynie małą usługę i rozmawia z nią własnym protokołem. To wygoda tam, gdzie SSH jest niewygodne; Node w tym trybie nie raportuje CPU i RAM-u i nie ma do niego terminala z aplikacji.

### Czym Node nie jest

Nie jest kontem w usłudze VibeSSH ani niczym, co my hostujemy. To Twoja maszyna, u Twojego dostawcy, z Twoim rachunkiem. VibeSSH nią zarządza i nic na niej nie trzyma poza tym, co sam każesz.

## Aplikacja

**Aplikacja to jedna rzecz, która działa na węźle.** Serwer Minecrafta, proxy Velocity, bot, baza. Jeden serwer gry to jedna aplikacja — nawet jeśli na tym samym węźle stoi ich pięć.

Aplikacja składa się z trzech warstw i to jest najważniejsze rozróżnienie w całym VibeSSH:

| Warstwa | Co to | Czy przeżywa |
| --- | --- | --- |
| Kontener | Proces i jego środowisko uruchomieniowe | **Nie.** Odtwarzany przy każdej zmianie konfiguracji. |
| Katalog roboczy | Świat, wtyczki, konfiguracja, logi | **Tak.** Leży na dysku węzła, poza kontenerem. |
| Deklaracja | Porty, limity, zmienne, obraz | **Tak.** Trzymana przez VibeSSH i nakładana na węzeł. |

> **Kontener jest jednorazowy, dane nie.** „Odtwórz kontener" brzmi groźnie i nie jest — usuwa i buduje na nowo warstwę pierwszą, nie ruszając drugiej. Twój świat Minecrafta leży w katalogu roboczym i nie ma go w kontenerze.

To wyjaśnia zachowanie, które inaczej wygląda na błąd: Docker zapisuje część konfiguracji w kontenerze **w chwili jego tworzenia**, a nie odczytuje jej przy starcie. Publikowane porty, limity zasobów, zmienne środowiskowe, obraz. Dlatego zmiana któregokolwiek z nich na działającej aplikacji **odtwarza kontener** — bo zwykły restart użyłby tego samego, nieaktualnego.

### Czym aplikacja nie jest

Nie jest kontenerem Dockera. Kontener jest sposobem, w jaki aplikacja akurat działa — zakładka Akcje pokazuje kontenery na węźle i można tam zatrzymać ten należący do aplikacji, ale zarządza się nią w jej własnym widoku, razem z konsolą, portami i backupami.

## Jak się mają do siebie

Aplikacja żyje **na dokładnie jednym węźle**. Przeniesienie jej gdzie indziej to migracja: dane jadą razem z nią, a źródło przestaje być jej właścicielem.

Węzeł ma **dowolnie wiele aplikacji**. Nie widzą się nawzajem, dopóki ich nie połączysz — każda dostaje własną sieć Dockera, a jeśli włączone jest konto dedykowane, także własne konto systemowe (`vibessh-app-…`). To nie jest kosmetyka: bez tego każda aplikacja czytałaby pliki każdej innej.

**Vibe Network łączy węzły, nie aplikacje.** To prywatny tunel WireGuard między maszynami, dzięki któremu aplikacja na jednym serwerze może sięgnąć do bazy na drugim, nie wystawiając jej portu do internetu. Sam tunel nie daje aplikacjom dostępu do siebie — to nadaje się osobno.

## Blueprint

**Blueprint to przepis na aplikację**: jaki obraz, jakie porty, jakie pliki konfiguracyjne. Punkt wyjścia, nie klatka — po utworzeniu każdą z tych rzeczy zmienisz. Jedyne, czego blueprint pilnuje na stałe, to porty oznaczone jako **Wymagane**: można je edytować, ale nie usunąć.

## Krótko

- **Node** — maszyna. Twoja, u Twojego dostawcy, obsługiwana po SSH.
- **Aplikacja** — jedna rzecz działająca na tej maszynie. Kontener jest jednorazowy, katalog roboczy nie.
- **Blueprint** — przepis, od którego zaczyna aplikację.
- **Vibe Network** — prywatna sieć **między węzłami**.
