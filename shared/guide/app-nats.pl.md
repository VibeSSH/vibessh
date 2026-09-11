---
id: app-nats
title: NATS
section: blueprints
route: /applications
order: 240
---

NATS to broker wiadomości. Aplikacje wysyłają do niego komunikaty i nasłuchują na nie, zamiast łączyć się bezpośrednio ze sobą.

Przydaje się, kiedy masz kilka usług, które muszą się dogadywać: serwer wysyła zdarzenie, bot je odbiera, panel wyświetla. Do jednej aplikacji nie jest potrzebny.

## Krok po kroku

1. **Aplikacje → Nowa aplikacja**, wybierz lokalizację i nazwę, np. `nats`.
2. Wybierz **NATS** z listy rodzajów.
3. **Wersja NATS** — zostaw `2`.
4. **Włącz JetStream (trwałość)** — zaznacz, jeśli wiadomości mają przetrwać restart brokera. Bez tego NATS jedynie przekazuje je dalej: kto akurat nie słucha, ten nie dostanie.
5. **Token uwierzytelniający** — ustaw, jeśli port będzie opublikowany.
6. Utwórz aplikację i kliknij **Uruchom**.

## Jak się z nim połączyć

Broker słucha na porcie **4222** wewnątrz kontenera.

1. Otwórz aplikację kliencką → zakładka **Porty** → karta **Połączenia** → połącz z aplikacją NATS.
2. W kliencie podaj adres `nats://NAZWA-APLIKACJI:4222`, gdzie nazwa jest zapisana małymi literami.
3. Jeśli ustawiłeś token, dopisz go zgodnie z dokumentacją swojej biblioteki klienckiej.

## Częste problemy

**Klient nie łączy się.** Sprawdź kartę **Połączenia** i adres — musi to być nazwa aplikacji, nie `localhost`.

**Wiadomości giną, kiedy odbiorca jest offline.** Tak działa NATS bez JetStreamu. Włącz **JetStream** w konfiguracji i użyj strumienia po stronie klienta.

**`authorization violation`.** Broker ma token, a klient go nie podaje albo podaje inny.
