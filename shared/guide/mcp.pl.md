---
id: mcp
title: Claude i inni asystenci
section: getting-started
route: /settings
order: 145
---

VibeSSH może odpowiadać asystentowi AI działającemu na Twoim komputerze — na przykład
Claude'owi w edytorze kodu albo w terminalu. Asystent pyta, jakie masz serwery i aplikacje,
i czyta ich logi. Dzięki temu na pytanie „czemu serwer nie wstaje?" patrzy na prawdziwy
log, zamiast zgadywać.

Domyślnie jest to **wyłączone** i nic nie nasłuchuje.

## Co asystent może zobaczyć

- nazwy serwerów, ich adresy, porty i login,
- listę aplikacji: rodzaj, katalog roboczy, status i to, na którym serwerze działają,
- ostatnie linie logu wybranej aplikacji.

## Czego nie zobaczy nigdy

- **haseł, kluczy prywatnych ani haseł do baz danych** — te zostają w magazynie kluczy
  systemu i nie opuszczają go nawet na Twoją prośbę,
- **zmiennych środowiskowych aplikacji** — także tych nieoznaczonych jako sekretne,
  bo w zwykłej zmiennej też potrafi wylądować klucz licencyjny.

## Jak to włączyć

1. **Ustawienia → Claude i inni asystenci**.
2. Włącz **Odpowiadaj asystentom na tym komputerze**.
3. Skopiuj **Adres** — wygląda tak: `http://127.0.0.1:7422/mcp`.
4. Kliknij **Pokaż** przy **Tokenie** i skopiuj go.

### Podłączenie w Claude Code

```bash
claude mcp add --transport http --scope user vibessh http://127.0.0.1:7422/mcp --header "Authorization: Bearer WKLEJ_TOKEN"
```

Trzy rzeczy, o które najczęściej się rozbija:

- **`--scope user`** sprawia, że serwer jest widoczny niezależnie od katalogu, w którym
  uruchamiasz Claude'a. Bez tego wpis należy tylko do katalogu, w którym go dodałeś —
  i w innym projekcie VibeSSH po prostu nie istnieje.
- **Lista serwerów MCP ustala się przy starcie sesji.** Po dodaniu wpisu zamknij bieżącą
  sesję i zacznij nową, bo narzędzia w tej starej się nie pojawią.
- **Token jest częścią wpisu.** Po wygenerowaniu nowego w VibeSSH trzeba podmienić go
  także tutaj — najprościej `claude mcp remove vibessh`, a potem komenda powyżej od nowa.

Sprawdzenie, co widzi Claude:

```bash
claude mcp list
```

Wpis `vibessh` powinien być na liście. Martwe wpisy z dawnych eksperymentów kasuje
`claude mcp remove NAZWA` — uruchomione z tego katalogu, w którym zostały dodane.

### Podłączenie gdzie indziej

Każdy klient MCP mówiący po HTTP potrzebuje tylko tych dwóch rzeczy: adresu
`http://127.0.0.1:7422/mcp` i nagłówka `Authorization: Bearer TOKEN`. Jeśli Twój klient
umie wyłącznie stdio, na razie go nie obsłużymy — mostek jest do dopisania, ale jeszcze
nie istnieje.

Na koniec zapytaj asystenta o coś prostego, na przykład „jakie mam serwery w VibeSSH?".

## Jak pozwolić na restartowanie

Osobny przełącznik **Zezwól na zmiany**, widoczny dopiero po włączeniu punktu wyżej.
Dopiero on daje asystentowi narzędzie do restartowania aplikacji.

Rozdzieliliśmy to celowo: „niech Claude widzi moje serwery" i „niech Claude je restartuje"
to dwa różne pytania i u większości ludzi mają różne odpowiedzi. Dopóki zmiany są
wyłączone, asystent **nie widzi nawet takiego narzędzia** — nie może go więc proponować.

## Odpalanie z IntelliJ (Gradle)

Zamiast lokalnego serwera deweloperskiego możesz po zbudowaniu wrzucić wtyczkę wprost na
serwer w VibeSSH i zrestartować go — bez wychodzenia z edytora.

Wymaga włączonego **Zezwól na zmiany**, bo zapisuje plik na serwerze.

1. Znajdź identyfikator aplikacji — zapytaj asystenta „jakie mam aplikacje w VibeSSH?".
2. Dopisz **dwa importy na samej górze** `build.gradle.kts`, przed blokiem `plugins`:

```kotlin
import java.net.HttpURLConnection
import java.net.URI
```

To nie jest ozdobnik. W `build.gradle.kts` nazwa `java` należy do rozszerzenia Gradle'a
(`JavaPluginExtension`), a nie do pakietu Javy — więc napisane wprost `java.net.URI`
kończy się błędem `Unresolved reference 'net'`. Importy sprawiają, że wystarczy
`URI` i `HttpURLConnection`.

3. Dopisz zadanie, gdziekolwiek w tym samym pliku:

```kotlin
val vibesshDeploy by tasks.registering {
    dependsOn(tasks.shadowJar) // albo tasks.jar
    doLast {
        val jar = tasks.shadowJar.get().archiveFile.get().asFile
        val app = providers.gradleProperty("vibesshApp").get()
        val token = providers.gradleProperty("vibesshToken").get()
        val url = URI(
            "http://127.0.0.1:7422/deploy?application=" + app +
                "&path=plugins/" + jar.name + "&restart=true"
        ).toURL()

        with(url.openConnection() as HttpURLConnection) {
            requestMethod = "POST"
            doOutput = true
            setRequestProperty("Authorization", "Bearer " + token)
            setRequestProperty("Content-Type", "application/octet-stream")
            outputStream.use { jar.inputStream().copyTo(it) }
            check(responseCode == 200) {
                "VibeSSH odmowil: " + responseCode + " " +
                    (errorStream?.readBytes()?.decodeToString() ?: "")
            }
            println("VibeSSH: " + inputStream.readBytes().decodeToString())
        }
    }
}
```

4. Rozdziel dwie wartości na dwa pliki. **To nie jest drobiazg** — jedna z nich jest
   sekretem, a druga nie.

   **Token** wpisz do swojego domowego `gradle.properties`, **poza projektem**:

   - Windows: `C:\Users\TWOJA-NAZWA\.gradle\gradle.properties`
   - Linux i macOS: `~/.gradle/gradle.properties`

   ```properties
   vibesshToken=WKLEJ-TOKEN
   ```

   Gradle czyta ten plik przy każdym projekcie, więc wystarczy raz.

   **Identyfikator aplikacji** może zostać w `gradle.properties` projektu — to nie jest
   sekret:

   ```properties
   vibesshApp=WKLEJ-ID-APLIKACJI
   ```

   Plik `gradle.properties` w katalogu projektu bardzo często **jest w repozytorium**.
   Token wpisany tam jest o jedno `git commit -a` od publicznego GitHuba, a token raz
   wypchnięty trzeba unieważnić, nie usunąć. Jeśli nie masz pewności, sprawdź:

   ```bash
   git ls-files --error-unmatch gradle.properties
   ```

   Odpowiedź bez błędu oznacza, że plik jest śledzony. Wtedy tym bardziej trzymaj token
   w pliku domowym, a `gradle.properties` projektu możesz dodatkowo dopisać do
   `.gitignore`.

5. W IntelliJ, w panelu **Gradle** (prawa krawędź), rozwiń
   `Tasks → other` i kliknij dwa razy `vibesshDeploy`.

Log po restarcie zobaczysz w VibeSSH w zakładce **Logi** tej aplikacji — albo poproś
o niego asystenta.

### Konfiguracja uruchomieniowa, żeby szło jednym skrótem

Dwuklikanie w panelu Gradle jest w porządku raz na jakiś czas. Przy pracy nad wtyczką
wygodniej mieć to pod jednym przyciskiem u góry okna.

1. W panelu **Gradle** kliknij prawym na `vibesshDeploy` → **Modify Run Configuration...**.
2. W **Name** wpisz coś czytelnego, na przykład `Deploy na bedwars`.
3. **Store as project file** zostaw **odznaczone**, jeśli w konfiguracji miałbyś cokolwiek
   prywatnego — zapisana konfiguracja projektu ląduje w `.idea/` i bywa commitowana.
4. **OK**. Konfiguracja pojawia się na liście obok przycisku ▶ w prawym górnym rogu.
5. Od teraz uruchamiasz ją skrótem **Shift+F10** (ostatnio używana konfiguracja).

Chcesz jeszcze krócej? W **Settings → Keymap** wyszukaj nazwę swojej konfiguracji
i przypisz jej własny skrót.

### Budowanie i wdrażanie jednym ruchem

Jeśli chcesz, żeby zwykłe **Build** od razu wysyłało wtyczkę, dopisz zależność zamiast
klikać dwa zadania:

```kotlin
tasks.named("build") { finalizedBy(vibesshDeploy) }
```

Ostrożnie z tym przy `restart=true` — każde zbudowanie projektu zrestartuje wtedy serwer,
łącznie z tymi budowaniami, które IntelliJ robi sam.

### Na co uważać

- **Token trafia do `gradle.properties`, czyli do pliku.** Trzymaj go poza repozytorium
  (dopisz do `.gitignore`) albo w `~/.gradle/gradle.properties`, wspólnym dla projektów.
- **`restart=true` restartuje prawdziwy serwer.** Przy produkcyjnym raczej ustaw `false`
  i zrestartuj świadomie, kiedy nikt nie gra.
- **Ścieżka jest względna wobec katalogu roboczego aplikacji.** Próba wyjścia poza niego
  jest odrzucana.
- **VibeSSH musi być uruchomiony.** Schowany do zasobnika wystarczy; zamknięty przez
  **Zakończ VibeSSH** już nie.

## Bezpieczeństwo, po ludzku

**To wejście nie wychodzi poza Twój komputer.** Nie da się tego zmienić w ustawieniach,
bo nie ma dobrego powodu, żeby lista Twoich serwerów była osiągalna z sieci.

**Token to hasło do tego wejścia.** Bez niego dostałby się tam każdy program działający na
tym komputerze. Traktuj go jak każde inne hasło — nie wklejaj na czat, nie pokazuj na
zrzucie ekranu. Dlatego domyślnie jest zasłonięty.

**Gdy token gdzieś ucieknie**, kliknij **Wygeneruj nowy token**. Stary przestaje działać
natychmiast, a nie przy następnym uruchomieniu — każdy asystent skonfigurowany starym
tokenem po prostu przestaje mieć dostęp.

## Jak sprawdzić, czy działa

- Zapytaj asystenta „jakie mam serwery w VibeSSH?" — powinien wymienić te z listy Serwery.
- Poproś o ostatnie linie logu jednej z aplikacji i porównaj z zakładką Logi w VibeSSH.

## Najczęstsze problemy

- **Asystent mówi, że nie może się połączyć** — sprawdź, czy VibeSSH jest uruchomiony.
  Wejście istnieje tylko wtedy, gdy aplikacja działa; po zamknięciu do zasobnika działa
  dalej, po **Zakończ VibeSSH** już nie.
- **„Nie udało się otworzyć wejścia … port zajęty"** — coś innego zajmuje 7422.
  Zmień port w ustawieniach i popraw adres w konfiguracji asystenta.
- **Asystent proponuje restart i dostaje odmowę** — masz wyłączone **Zezwól na zmiany**.
- **Asystent nie widzi nowego tokenu** — po wygenerowaniu nowego trzeba go wkleić
  ponownie po stronie asystenta; stary jest unieważniony od razu.
- **`Unresolved reference 'net'` przy `java.net.URI`** — w `build.gradle.kts` nazwa `java`
  należy do rozszerzenia Gradle'a, nie do pakietu Javy. Dodaj `import java.net.URI`
  i `import java.net.HttpURLConnection` na samej górze pliku i używaj krótkich nazw.
- **Token trafił do `gradle.properties` w projekcie** — sprawdź, czy plik jest śledzony
  (`git ls-files --error-unmatch gradle.properties`). Jeśli tak, wygeneruj nowy token
  w VibeSSH i wpisz go do domowego `~/.gradle/gradle.properties`. Stary unieważnia się
  sam w chwili wygenerowania nowego.
- **Claude nie widzi narzędzi po `claude mcp add`** — lista serwerów ustala się przy
  starcie sesji. Zakończ bieżącą i zacznij nową.
