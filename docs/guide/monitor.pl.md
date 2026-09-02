---
id: monitor
title: Monitor
section: nodes
route: /monitor
order: 65
---

Monitor pokazuje, co węzeł robi w tej chwili: zużycie zasobów, ruch sieciowy i listę procesów. To odczyt na żywo, a nie system historii — dane zbierają się, dopóki strona jest otwarta.

## Gdzie to jest

Menu boczne → **Monitor**, po wybraniu serwera.

## Zasoby

Cztery odczyty, odświeżane co kilka sekund: **CPU**, **RAM**, **Sieć — odbiór** i **Sieć — wysyłka**.

CPU i przepustowość sieci to **różnice między dwoma odczytami**, a nie wartości, które system podaje wprost. Dlatego pierwszy odczyt po wejściu pokazuje „zbieranie danych…" — nie ma jeszcze z czym go porównać.

## Historia

Wykres ostatnich kilkunastu minut, budowany od momentu otwarcia strony. Nic go nie zapisuje: po wyjściu i powrocie zaczyna się od nowa. Wykres, który udawałby przeszłość, której nikt nie rejestrował, byłby gorszy niż pusty.

## Procesy

Lista uruchomionych procesów posortowana po zużyciu pamięci, z PID-em, użytkownikiem, CPU, RAM-em i poleceniem.

Sortowanie po pamięci jest celowe: proces zjadający pamięć to najczęstsza przyczyna tego, że węzeł nagle zwalnia albo że jądro zabija serwer gry.

## Odświeżanie

Strona odpytuje węzeł co kilka sekund i **przestaje, gdy okno jest schowane**, a rusza od razu po powrocie. Każdy odczyt to połączenie SSH — monitor zostawiony na noc bez tego kosztowałby tysiące zbędnych połączeń.

## Częste pomyłki

- **Zamknąłem stronę i historia przepadła** — tak ma być, nic jej nie utrwala. Do obserwacji długoterminowej potrzebne jest osobne narzędzie na węźle.
- **CPU pokazuje 0%** — to najpewniej pierwszy odczyt. Poczekaj na drugi.
- **RAM wygląda na zajęty w całości** — Linux używa wolnej pamięci na cache dyskowy i oddaje ją, gdy jest potrzebna. Interesująca jest pamięć zajęta przez procesy, a nie „wolna".
