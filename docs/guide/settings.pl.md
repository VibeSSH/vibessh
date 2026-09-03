---
id: settings
title: Ustawienia
section: getting-started
route: /settings
order: 140
---

Ustawienia zbierają opcje dotyczące całej aplikacji.

## Jak zmienić język

1. Otwórz **Ustawienia**.
2. W karcie **Preferencje** wybierz **Język**.

Ustawienie działa tylko na tym urządzeniu.

## Jak ustawić cel backupów (S3)

1. Znajdź kartę **Cel backupów**.
2. Zaznacz **Wysyłaj backupy do zewnętrznego magazynu**.
3. **Endpoint** — adres magazynu.
4. **Region**, **Bucket** — dane z panelu magazynu.
5. **Access Key ID** i **Secret Access Key** — klucze dostępu.
6. Przy MinIO zaznacz **Adresowanie path-style**.
7. Kliknij **Testuj połączenie**.
8. Zapisz.

## Jak dodać prywatny rejestr Docker

1. Znajdź kartę **Prywatne rejestry Docker**.
2. Kliknij **Dodaj rejestr**.
3. **Adres rejestru** — np. `ghcr.io`.
4. **Nazwa użytkownika** i **Hasło / token dostępu**.
5. Zapisz.

Obrazy publiczne działają bez logowania.

## Jak zmienić sufiks DNS

1. Znajdź kartę **Sufiks DNS**.
2. Wpisz nową końcówkę, np. `vibe`.
3. Zapisz.
4. Otwórz **Vibe Network** i kliknij **Synchronizuj Vibe Network**.

## Jak sprawdzić, czy działa

- Interfejs zmienia język od razu.
- Test połączenia z magazynem kończy się powodzeniem.
- Karta **O aplikacji** pokazuje wersję i odpowiedź backendu.

## Najczęstsze problemy

- **Backupy nie trafiają do S3** — użyj **Testuj połączenie**. Przy MinIO najczęstszą przyczyną jest niezaznaczone **Adresowanie path-style**.
- **Wyczyściłem pole klucza, żeby go usunąć, a nic się nie zmieniło** — puste pole zachowuje obecny klucz.
- **„Oczekiwanie na backend"** — interfejs działa, ale nic pod nim. Zrestartuj aplikację.
