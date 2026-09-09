# Changelog

What changed in each release, written for the person using VibeSSH rather
than for the person who wrote it. The commit history has the reasoning.

This file starts at 0.1.0-beta.5. Earlier releases were published without one;
their contents are in the git history between the tags.

## 0.1.0-beta.7

### Fixed

- **On Linux the interface was drawing every frame on the CPU.** Someone on
  Fedora with a Ryzen 5600G and a Radeon RX 570 reported an interface running
  at something like five frames per second, while the task manager showed the
  process using nothing - because nothing on the CPU was the bottleneck. Two
  causes, both ours. VibeSSH turned WebKitGTK's accelerated renderer off on
  *every* Linux machine, to avoid a crash that belongs to NVIDIA's driver; it
  now looks for NVIDIA's kernel module and does that only there. And every
  server card carried a blur of the background behind it - a background which
  is one flat colour, so the blur returned the colour it started with and cost
  a great deal per card to do it.
- **Error messages arrive in your language.** The six error codes almost
  everything is reported through had no translation, so a Polish interface
  showed sentences like "unauthorized: not signed in to the VibeSSH cloud
  backend". They are translated now, and not being signed in has a sentence of
  its own that says what to do about it.
- **Accounts being off no longer reads as accounts being broken.** Settings
  painted the untouched default backend address in red and said signing in
  would fail. For somebody using VibeSSH alone that is a warning about
  nothing - nodes, applications, files and the terminal need no account at
  all. The warning now appears in the sign-in dialog, above the fields, rather
  than in Settings and after a failed attempt.

### Changed

- **Creating an application on this computer offers the runtime that needs
  nothing first.** The wizard listed Docker first, because that is what
  blueprints are written for - so the option people reached for on their own
  Windows machine was the one needing Docker Desktop, WSL2 and a restart,
  rather than the one beside it that installs nothing and downloads its own
  Java. Docker is still there and still selectable.
- **A local application suggests where to put its files.** The field was empty
  and required, so the first thing anybody did was invent a path, and what
  they invented was the Desktop - which then held a server's worlds and logs.
  It now suggests a directory inside the app's own data folder.

## 0.1.0-beta.6

### Fixed

- **Accounts and teams could not work at all.** The address of the backend
  they talk to is a per-install setting, and it defaulted to a server on your
  own computer - but nothing in the interface let you change it, so every
  attempt to register or sign in failed against `http://localhost:8787`.
  There is now an **Account backend** card in Settings, and the error says
  which situation you are in rather than naming a URL you never chose.
- **"Docker isn't running" now says so.** Starting an application with Docker
  Desktop stopped reported a missing named pipe, which is an accurate
  description of the wrong thing - people went looking for a path. The
  message now says to start Docker Desktop, and keeps the original text after
  it.

### New

- **A guide page on running your own backend**, reachable from the question
  mark on that Settings card: when you need one at all (most people do not -
  nodes, applications, files and the terminal need no account), how to start
  it, and how to back it up.

## 0.1.0-beta.5

### New

- **A waiting update now says so.** The app has always checked on its own,
  five seconds after start and every six hours after that, but announced it
  only by putting a dot on a small cloud icon. There is now a notice across
  the top of the page. Hiding it means "not this version" - the next release
  brings it back.
- **A right-click menu in the file editors.** Copy, cut, paste and select all,
  in both the Application Files editor and the Node Files editor. Right
  clicking there previously did nothing at all.
- **Sliding tabs and animated dialogs.** The underline follows the selected
  tab instead of jumping, and dialogs arrive rather than appearing. Both
  respect the system's "reduce motion" setting.

### Fixed

- **The database file now holds the data.** Recent writes were living in
  SQLite's write-ahead log: on a real install the database file was 4 KB while
  the log beside it was 2.6 MB. Anything that copied the database file alone -
  a backup, a folder sync, moving your profile to another machine - was
  copying almost nothing. The log is now folded in when the app closes
  cleanly.
- **Servers in the Add Node dialog said `rail.statusOnline`** instead of
  "Online". Every picker that lists servers was affected.
- **The mouse cursor is a pointer over things that are clickable.** It was the
  ordinary arrow over any button whose own stylesheet had not set it, which
  was most of them.

### Under the hood

- The frontend toolchain runs on **Bun** instead of npm. Dependencies install
  in about a fifth of the time; Vite still builds and vitest still tests.
  Contributors need Bun (`winget install Oven-sh.Bun`) - see the README.

## 0.1.0-beta.4

### New

- **An application can change what it is.** Switching between a managed type
  (Paper, Purpur, Velocity, Waterfall) and a plain Docker container, in both
  directions. Taking over management downloads that server's own jar; giving
  it up touches nothing on disk, and the warning says which you are doing.
- **Templates that ship with the app.** MariaDB with a root password, and two
  for phpMyAdmin. A blueprint says which image to run; it does not say which
  variables that image refuses to start without, and these do.
- **phpMyAdmin connects to a database by picking one.** It was the most
  reported broken setup in the app, and it broke in three places at once: the
  host unset, the port unset, and - the part nobody guesses - the two
  containers on separate private networks. Picking the database fills in all
  three.
- **MongoDB**, alongside MariaDB and Redis.
- **A command console for Redis and MongoDB.** The existing console types into
  a process's standard input, which a database ignores; this runs the server's
  own client and shows what it answered.
- **A guide page for every kind of application**, in Polish and English, with
  a question mark next to the application's type that opens the right one.
- **A plain `.tar.gz` for Linux**, for machines where neither the `.deb` nor
  the AppImage fits.

### Fixed

- **Databases on a node are reachable from containers.** The server was bound
  to loopback only, and the node's firewall dropped the container's packet
  silently - which is why a correct password produced a timeout rather than a
  refusal. There is also a "Fix container access" button for hosts already in
  that state.
- **Two applications on this computer can be connected to each other.** The
  rule said a Docker network does not span hosts, and then rejected two
  containers that were both on this one.
- **Long guide pages scroll to the end.** The page was measured before its
  screenshots had loaded, so the last few hundred pixels were unreachable.
