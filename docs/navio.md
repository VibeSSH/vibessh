# Słownik
- NODE - fizyczna maszyna z zainstalowanym agentem vibe-ssh
- APPLICATION - kontener docker (póki co zrobiłbym tylko kontenery bez innych opcji) działający na danym NODE 

## Co bym dodał:

## Co trzeba dodać:
### Setup page 
Setup page: sekcja, która wyświetla się podczas instalacji serwera (Node'a):
- instalacja vibessh-agenta, który jest kluczowy do pełnego działania node'a
- instalacja wymaganych pakietów: wireguard, docker, UFW (opcjonalnie jeśli user chce zainstalować sam lub już jest zainstalowane, powinno sprawdzić czy jest połączenie w taki sam sposób jaki normalnie się łączy e.g., przez komende w konsoli)
- zabezpieczenie serwera (włączenie defaultowych ustawień UFW, które blokują wszystkie połączenia, z wyłączeniem SSH żeby nas nie odłączyło) - domyślnie jako włączone
### Readme/docsy
- requirements w readme: docker, wireguard, UFW (napisać że są instalowane za pomocą instalatora jeśli ich nie ma)


# Główne założenia/feature'y

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