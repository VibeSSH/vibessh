---
id: ports
title: Porty
section: applications
route: /applications
order: 30
---

Port to deklaracja, z których gniazd sieciowych korzysta aplikacja i kto może się do nich połączyć. VibeSSH nie zgaduje tego z kontenera — deklarujesz to sam, a węzeł jest doprowadzany do tego stanu przy każdej zmianie.

## Gdzie to jest

Aplikacja → zakładka **Porty**. Karta na górze wymienia zadeklarowane porty, a pod nią, za kreską, siedzi synchronizacja firewalla węzła.

## Pola formularza

| Pole | Co robi | Przykład |
| --- | --- | --- |
| Nazwa | Etykieta na liście. Nie trafia nigdzie do konfiguracji serwera. | `Minecraft` |
| Protokół | TCP albo UDP. Minecraft Java to TCP, Bedrock i większość gier na Source to UDP. | `TCP` |
| Dostęp | Kto może się połączyć. Opisane niżej. | `Publiczny` |
| Port wewnętrzny | Port, na którym nasłuchuje proces **w kontenerze**. | `25565` |
| Port zewnętrzny | Port na węźle. Zostaw pusty, żeby użyć tego samego numeru. | `25566` |

Port zewnętrzny przydaje się, gdy dwie aplikacje na jednym węźle nasłuchują wewnątrz na tym samym numerze. Dwa serwery Paper mogą oba mieć `25565` wewnątrz, a na zewnątrz `25565` i `25566` — kontener nie musi o tym wiedzieć.

## Dostęp — cztery poziomy

To jest pole, które decyduje o bezpieczeństwie, więc plakietka przy porcie jest kolorowa: publiczny świeci ostrzegawczo, Vibe Network na zielono, reszta neutralnie.

- **Publiczny** — dowiązanie do `0.0.0.0`, dostępny z całego internetu. To, czego chcesz dla portu gry, do którego łączą się gracze.
- **Vibe Network** — dostępny wyłącznie z adresu węzła w prywatnej sieci. Panel administracyjny albo API, do którego łączysz się sam, powinno być tutaj, a nie publicznie.
- **Localhost** — dowiązanie do `127.0.0.1`. Widoczne tylko dla procesów na tym samym węźle.
- **Własny adres** — podajesz adres dowiązania ręcznie. Dla przypadków, których trzy powyższe nie obejmują.

> Sam wybór dostępu nie zamyka portu przed światem — robi to firewall węzła. Dlatego po zmianie dostępu warto kliknąć **Zsynchronizuj firewall**, jeśli nie chcesz czekać.

## Co się dzieje po zapisaniu

Opublikowane porty Dockera są zapisane w kontenerze w momencie jego utworzenia (`docker create -p`), a nie odczytywane przy starcie. Zwykły restart użyłby tego samego, nieaktualnego kontenera, więc **zmiana portu na działającej aplikacji odtwarza kontener**. Aplikacja zatrzymana zostaje zatrzymana.

Odtworzenie kontenera nie rusza danych — pliki aplikacji żyją poza kontenerem.

## Synchronizacja firewalla węzła

Przycisk na dole karty. Wylicza reguły dla wszystkich aplikacji na tym węźle i nakłada je: port SSH, port WireGuarda i każdy port aplikacji zgodnie z jego poziomem dostępu. Reguły, które przestały być potrzebne, są usuwane.

Wynik potrafi powiedzieć trzy niewygodne rzeczy i warto je rozpoznać:

- **Brak backendu firewalla** — węzeł nie ma ani `ufw`, ani `nftables`. Nic nie egzekwuje reguł.
- **Reguły nieegzekwowane** — backend jest, ale nieaktywny. To najgorszy przypadek, bo synchronizacja „się udała", a port oznaczony jako „tylko Vibe Network" jest dostępny publicznie.
- **Węzeł nieosiągalny** — reguł nie nałożono wcale.

## Porty wymagane

Port oznaczony jako **Wymagany** pochodzi z blueprintu aplikacji. Można go edytować, ale nie usunąć — tę samą zasadę wymusza backend, nie tylko interfejs.

## Częste pomyłki

- **Port działa lokalnie, nie działa z zewnątrz** — sprawdź dostęp na `Publiczny` i zsynchronizuj firewall. Jeśli mimo to nie działa, sprawdź firewall u dostawcy VPS-a; VibeSSH nie ma do niego dostępu.
- **„Port jest zajęty" przy zapisie** — inna aplikacja lub proces na węźle trzyma ten numer. Zmień port zewnętrzny.
- **Zmieniłeś port, gracze dalej trafiają na stary** — zmiana odtwarza kontener tylko wtedy, gdy aplikacja działa. Jeśli była zatrzymana, kontener powstanie z nową konfiguracją przy starcie.
