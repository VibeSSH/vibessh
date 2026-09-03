---
id: concepts
title: Podstawowe pojęcia
section: getting-started
route: /
order: 0
---

Krótki słownik nazw, które wracają w całym poradniku.

## Node

Serwer lub VPS dodany do VibeSSH. Jedna maszyna to jeden Node.

## Aplikacja

Jedna rzecz działająca na Node: serwer Minecraft, proxy, bot, baza. Na jednym Node może działać wiele aplikacji.

## Obraz

Gotowy przepis, z którego tworzysz aplikację. W kreatorze pole nazywa się **Obraz** (np. Paper, Velocity).

## Katalog roboczy

Folder na Node, w którym leżą pliki aplikacji: świat, wtyczki, konfiguracja. Znajdziesz go w zakładce **Pliki**.

## Port

Numer, pod którym usługa jest dostępna. Minecraft to zwykle `25565`.

## Vibe Network

Prywatna sieć między Twoimi Node'ami. Pozwala aplikacjom na różnych serwerach łączyć się bez wystawiania portów do internetu.

## Firewall

Kontroluje, które porty Node są dostępne z internetu.

## Więcej informacji

Aplikacja składa się z dwóch części: kontenera (proces) i katalogu roboczego (dane). Kontener jest odtwarzany przy zmianie konfiguracji, katalog roboczy zostaje nienaruszony. Dlatego **Odtwórz kontener** nie kasuje świata ani wtyczek.
