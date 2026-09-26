---
id: application-access
title: Kto widzi aplikację
section: applications
route: /applications
order: 60
---

Udostępnienie aplikacji zespołowi pokazuje ją wszystkim w tym zespole. Zakładka **Użytkownicy** zawęża to do wskazanych osób: kto nie został dodany, w ogóle nie widzi aplikacji na swojej liście.

## Zanim zaczniesz

To korzysta z Twojego konta VibeSSH i zespołu. Jeśli nie jesteś zalogowany albo nie masz jeszcze zespołu, zakładka Ci to powie i wskaże, gdzie to ustawić. Dostęp nadaje się osobom, które są członkami Twojego zespołu.

## Jak nadać komuś dostęp

1. Otwórz aplikację i przejdź do zakładki **Użytkownicy**.
2. Jeśli należysz do więcej niż jednego zespołu, wybierz zespół na górze.
3. Znajdź osobę na liście i włącz przełącznik przy jej nazwie.

Pierwsza dodana osoba zmienia to, co „udostępnione" znaczy dla tej aplikacji. Do tego momentu widzi ją cały zespół; od pierwszego włączonego przełącznika widzą ją tylko osoby, które włączyłeś, oraz Ty, bo to Ty ją udostępniłeś. Nota nad listą zawsze mówi, który z tych dwóch stanów jest teraz aktualny.

## Jak dać komuś uprawnienia do tej aplikacji

Samo włączenie przełącznika pozwala oglądać aplikację i czytać jej logi. Żeby ktoś mógł coś z nią zrobić, zaznacz uprawnienia pod jego nazwą:

- **Uruchamianie i zatrzymywanie** - start, stop, restart i wymuszenie zakończenia.
- **Konsola (wpisywanie komend)** - wpisywanie komend do konsoli. Na serwerze Minecraft to każda komenda, także `op`, więc dawaj to tylko zaufanym osobom.
- **Odczyt plików** - przeglądanie, otwieranie i pobieranie plików.
- **Edycja i wysyłanie plików** - zmiany w plikach. Zaznaczenie tego zaznacza też odczyt.

Te uprawnienia dotyczą tylko tej jednej aplikacji. Kolega z restartem na jednym serwerze nie ruszy drugiego, który stoi na tym samym Nodzie.

Po zmianie uprawnień otwórz **Zespoły**, swój zespół, sekcję **Serwery** i kliknij synchronizację dostępu przy Nodzie, na którym działa aplikacja. Dopiero wtedy uprawnienia trafiają na serwer.

## Co widzi osoba, której coś udostępniłeś

1. Ta osoba otwiera **Zespoły**, wasz zespół i sekcję **Serwery**, a przy Nodzie klika **Dodaj do moich serwerów**. VibeSSH łączy się wtedy jej własnym kontem, nie Twoim.
2. Na jej liście **Aplikacje** pojawia się Twoja aplikacja z oznaczeniem **Udostępniona**.
3. Po otwarciu widzi tylko to, na co jej pozwoliłeś: podgląd, logi, a do tego przyciski i zakładki z zaznaczonych uprawnień. Na górze strony jest napisane, co może.

Nie może jej usunąć, zmienić ustawień, portów ani kopii zapasowych - to zawsze zostaje przy Tobie.

## Jak odebrać dostęp

Wyłącz przełącznik przy nazwie osoby. Gdy wyłączysz ostatni, aplikacja wraca do widoczności dla całego zespołu - lista jest znów pusta i nota to potwierdza.

## Jak sprawdzić, że działa

- Gdy dodasz co najmniej jedną osobę, nota nad listą pokazuje **Widoczna tylko dla wskazanych osób**.
- Osoby, które włączyłeś, mają przełącznik włączony; reszta wyłączony.

## Częste problemy

- **Zakładka mówi, że wymaga zaktualizowanego backendu** - Twój backend kont nie ma jeszcze tej funkcji. Zacznie działać sam, gdy go zaktualizujesz; nic tu nie jest zepsute.
- **Nie mogę włączyć żadnego przełącznika** - nie masz uprawnień do zarządzania udostępnianiem w tym zespole. Może Ci je nadać ktoś, kto je ma.
- **Jestem jedyną osobą na liście** - najpierw zaproś ludzi do zespołu. Dostęp nadaje się członkom zespołu.
- **Kolega nie widzi aplikacji na swojej liście** - sprawdź, czy dodał Node do swoich serwerów przez stronę zespołu, a nie ręcznie innym kontem. Aplikacja pojawia się tylko tam, gdzie łączy się własnym kontem.
- **Kolega dostaje komunikat, że nie ma uprawnienia** - uprawnienie nie jest zaznaczone albo nie było jeszcze synchronizacji dostępu po jego zaznaczeniu.

## Warto wiedzieć

Uprawnienia przy aplikacji pilnuje sam serwer: konto kolegi na Nodzie może wykonać dokładnie te polecenia, które mu dałeś, i nic więcej, także poza VibeSSH. Sama widoczność na liście to co innego - ktoś, kto ma dostęp do serwera przez SSH, i tak może zobaczyć, że aplikacja tam jest.
