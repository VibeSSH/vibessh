# Słownik
- NODE - fizyczna maszyna z zainstalowanym agentem vibe-ssh
- APPLICATION - kontener docker (póki co zrobiłbym tylko kontenery bez innych opcji) działający na danym NODE 

## Co bym dodał:
- Plugin gradle do przesyłu zbuildowanego .jar do danej lokalizacji `node_name`.`app_name`.`/container/path/to/file` wraz ze skryptem restartującym aplikacje. Powinien być generalny, nie tylko do papera czy rozwiązań mc. Nie mam pojecią jak mógłby póki co wyglądać taki przesył, może korzystałoby to z .exe/bin i tam wykonywało komende, ale to też ma swoje ograniczenia (takie podawanie argumentów do exe). 
- Backupy: support S3 (cloudflare R2, to ten sam protokół), https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/
- Dodanie auto usuwania backupów przy danej ilości lub danym czasie (i jeszcze ewentualnie po przekroczeniu ilości GB)

## Co trzeba dodać:
### Aplikacje
- pokazanie jakiego image aktualnie używa aplikacja
- możliwość aktualizacji/zmiany image
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
- Działające wnętrzne aplikacji (kontener) jest **efemeryczny** (ephemeral) co oznacza, że tylko konfiguracja i dane które mają być zapisywane między restartami są ważne i zostaną zapisane. Wszystko to co jest w kontenerze i nie jest przypisane do volume może być usunięte (default docker behavior).
- **WAŻNE!** nazwa każdej aplikacji musi przestrzegać 'RFC 1123 DNS' `^(?=.{1,63}$)[a-z0-9]([-a-z0-9]*[a-z0-9])?$`
- Posiada swój domyślny własny FQDN `nazwa_aplikacji`.`nazwa_node`.vibe
- Aplikacja ma przypisany image, może być z docker hub ale nie musi. Powinna być możliwość instalowania image'ów z private source, czyli trzeba podać tokeny np do docker huba (to jest raczej jeden ten sam protokół). Tokeny do image repo są wrażliwe i powinny być zapisywane z security in mind. Image aplikacji może być zaktualizowany/zamieniony, bez konieczności resetowania/usuwania całej konfiguracji aplikacji.

## Terminal i File System
- połączenie z STDIN/STDOUT kontenera/procesu i łatwe korzystanie z konsoli
- połaczenie z systemem plików serwera/aplikacji/kontenera i możliwość przesyłania/usuwania/zmieniania plików w łatwy sposób
- **fajnie by było** gdyby zapamiętywało (sczytywało) ostatnie logi, a nie tak jak w ptero, że nagle połowa znika od samej góry. I może nawet zapamiętywało logi między odpaleniami (może być w jakimś temp, czy w cache'u UI nawet)

## Vibe DNS (coś jak Magic DNS w Tailscale oraz DNS w cloudach, FQDN)
- rekordy DNS są synchronizowane między node'ami oraz aplikacjami w czasie rzeczywistym
- domena `.vibe` powinna być konfigurowalna i powinna przestrzegać  `^(?=.{1,127}$)(?!-)[a-zA-Z0-9-]{1,63}(?<!-)(\.(?!-)[a-zA-Z0-9-]{1,63}(?<!-))*$` (wyklucza maks. 2x 63 znaki i przestrzega zasad FQDN, czyli może być np. mynetwork.local)

## Vibe Firewall
- używa UFW
- synchronizacja między wszystkimi node'ami w czasie rzeczywistym
- domyślnie blokuje wszystkie porty oprócz SSH
- umożliwia tworzenie exit portów (porty otwarte na świat). Exit port można podłączyć do aplikacji a system sam stwierdzi na którym node trzeba wystawić port. Nazwa exit portu musi przestrzegać 'RFC 1123 DNS'. Podczas podłączania exit portu do aplikacji trzeba określić wewnętrzny port aplikacji (tak jak domyślnie przy kontenerach np 25565:25577, gdzie 25577 to port wewnętrzny który wystawia aplikacja w kontenerze)

## Port forwarding
- SSH port forwarding https://dev.to/bbkr/ssh-port-forwarding-from-within-rust-code-5an, https://www.digitalocean.com/community/tutorials/ssh-port-forwarding

# Zaawansowane - na przyszłość, do przemyślenia
- zintegrowanie systemu logów tak aby zbierały się w jedno miejsce (Prometheus, Grafana, OpenTelemetry tego typu sprawy).
- dodanie integracji z Sentry (bardziej taka upo wersja patrzenia poprzez logi .json niż jakieś stacktrace'y czy coś) lub nawet zrobienie czegoś swojego prostego w tym stylu