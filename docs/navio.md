# Słownik
- NODE - fizyczna maszyna z zainstalowanym agentem vibe-ssh
- APPLICATION - kontener docker (póki co zrobiłbym tylko kontenery bez innych opcji) działający na danym NODE 

## Co bym dodał:
- Plugin gradle do przesyłu zbuildowanego .jar do danej lokalizacji `node_name`.`app_name`.`/container/path/to/file` wraz ze skryptem restartującym aplikacje. Powinien być generalny, nie tylko do papera czy rozwiązań mc. Nie mam pojecią jak mógłby póki co wyglądać taki przesył, może korzystałoby to z .exe/bin i tam wykonywało komende, ale to też ma swoje ograniczenia (takie podawanie argumentów do exe). 

## Co trzeba dodać:
### Setup page 
Setup page: sekcja, która wyświetla się podczas instalacji serwera (Node'a):
- instalacja vibessh-agenta, który jest kluczowy do pełnego działania node'a
- instalacja wymaganych pakietów: wireguard, docker, UFW (opcjonalnie jeśli user chce zainstalować sam lub już jest zainstalowane, powinno sprawdzić czy jest połączenie w taki sam sposób jaki normalnie się łączy e.g., przez komende w konsoli)
- zabezpieczenie serwera (włączenie defaultowych ustawień UFW, które blokują wszystkie połączenia, z wyłączeniem SSH żeby nas nie odłączyło) - domyślnie jako włączone
### Readme/docsy
- requirements w readme: docker, wireguard, UFW (napisać że są instalowane za pomocą instalatora jeśli ich nie ma)


# Główne założenia/feature'y

## Ease of use, one click installation
- Instalacja na desktopie wymaga wpisania jednej komendy do powershell'a lub po prostu użycia szybkiego instalatora. Może nawet udałoby się zrobić wersje portable lub instalacje za pomocą NPX.
- Instalacja na serwerze wymaga połączenia poprzez aplikacje, która zainstaluje vibe-agent i przeprowadzi nas przez setup.

## Czym się to różni od ptero?
- Brak centralizacji.
- **Brak konieczności hostowania panelu**.
- Używamy kontenerów docker (w przyszłości także innych rozwiązań) zamiast EGG'ów, które są dużym ograniczeniem.
- UI jest odpalane z localhosta i łączymy się poprzez SSH i dedykowanego użytkownika. Nie ma potrzeby hostowania całej strony i robienia SSL i innych rzeczy, które utrudniają zarządzanie serwerami.
- Nasze wykorzystanie połączenia p2p bez konieczności korzystania z proxy takiego jak Cloudflare umożliwia przesyłanie dużych plików i wykorzystania całego łącza.
- Dzięki **Vibe DNS** i **Vibe Firewall** tworzymy całą sieć która współgra jak w rozwiązaniach cloud, ale w mniejszej skali (co w tym przypadku jest dużo lepsze).
- Pterodactyl skupia się bardziej na zarządzaniu pojedynczymi serwerami. VibeSSH skupia się na zarządzaniu całą siecią lub pojedynczym serwerem.

## Node
- **WAŻNE!** nazwa każdego node musi przestrzegać 'RFC 1123 DNS' `^(?=.{1,63}$)[a-z0-9]([-a-z0-9]*[a-z0-9])?$`
- Posiada swój domyślny własny FQDN `nazwa_node`.vibe

## Aplikacja
- Aplikacja jest przypisana do pojedynczego node'a
- **WAŻNE!** nazwa każdej aplikacji musi przestrzegać 'RFC 1123 DNS' `^(?=.{1,63}$)[a-z0-9]([-a-z0-9]*[a-z0-9])?$`
- Posiada swój domyślny własny FQDN `nazwa_aplikacji`.`nazwa_node`.vibe

## Vibe DNS (coś jak Magic DNS w Tailscale oraz DNS w cloudach, FQDN)
- rekordy DNS są synchronizowane między node'ami oraz aplikacjami w czasie rzeczywistym
- domena `.vibe` powinna być konfigurowalna i powinna przestrzegać  `^(?=.{1,127}$)(?!-)[a-zA-Z0-9-]{1,63}(?<!-)(\.(?!-)[a-zA-Z0-9-]{1,63}(?<!-))*$` (wyklucza maks. 2x 63 znaki i przestrzega zasad FQDN, czyli może być np. mynetwork.local)

## Vibe Firewall
- używa UFW
- synchronizacja między wszystkimi node'ami w czasie rzeczywistym
- domyślnie blokuje wszystkie porty oprócz SSH
- umożliwia tworzenie exit portów (porty otwarte na świat). Exit port można podłączyć do aplikacji a system sam stwierdzi na którym node trzeba wystawić port. Nazwa exit portu musi przestrzegać 'RFC 1123 DNS'. Podczas podłączania exit portu do aplikacji trzeba określić wewnętrzny port aplikacji (tak jak domyślnie przy kontenerach np 25565:25577, gdzie 25577 to port wewnętrzny który wystawia aplikacja w kontenerze)