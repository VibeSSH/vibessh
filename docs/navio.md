## Co bym dodał:

## Co trzeba dodać:
### Setup page 
Setup page: sekcja, która wyświetla się podczas instalacji serwera (Node'a):
- instalacja vibessh-agenta, który jest kluczowy do pełnego działania node'a
- instalacja wymaganych pakietów: wireguard, docker, UFW (opcjonalnie jeśli user chce zainstalować sam lub już jest zainstalowane, powinno sprawdzić czy jest połączenie w taki sam sposób jaki normalnie się łączy e.g., przez komende w konsoli)
- zabezpieczenie serwera (włączenie defaultowych ustawień UFW, które blokują wszystkie połączenia, z wyłączeniem SSH żeby nas nie odłączyło)
### Readme/docsy
- requirements w readme: docker, wireguard (i napisać ż)