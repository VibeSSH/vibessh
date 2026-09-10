# Changelog

What changed in each release, written for the person using VibeSSH rather
than for the person who wrote it. The commit history has the reasoning.

This file starts at 0.1.0-beta.5. Earlier releases were published without one;
their contents are in the git history between the tags.

## 0.1.0-beta.9

### Fixed

- **A command typed into an application's console no longer just sits there.**
  The console feeds the container through a pipe that a background `docker
  attach` reads, and that attach belongs to one running instance of the
  container - so a restart, whether Docker's own after a crash or a start from
  outside VibeSSH, left the pipe with nobody reading it. Writing to a pipe
  nobody is reading waits forever, which is exactly what the interface did.
  The console now gives it five seconds, reattaches, and tries again, so the
  usual case fixes itself. When it genuinely cannot be fixed from there - a
  container created without an interactive stdin can never gain one - it says
  so, and says that Recreate is the answer.
- **Ctrl+C copies in the application console.** Selecting a stack trace and
  pressing it did nothing at all: the terminal swallowed the keystroke and
  only the right-click menu could copy. Plain Ctrl+C here, deliberately unlike
  the SSH terminal, where it has to keep reaching the shell as "interrupt" -
  this console has no shell to reach, since commands go through the field
  below it.

### Changed

- **Switching between applications lands where you left off.** The tab strip
  is there to make going back and forth one click; it was one click to the
  application and three more back to the tab you had been on, every time.
  Which tab each application was last showing is remembered now. A link to a
  particular tab still wins, so the guide's links go where they say.

## 0.1.0-beta.8

### New

- **The Logs tab can go back to the beginning.** Five hundred lines of a
  server that has just fallen over is a long way to drag a scrollbar, and the
  line that explains it is usually the first one rather than the last. Both
  ends are one press now.
- **Clearing the logs keeps a copy.** The history VibeSSH captures survives
  restarts and container recreates - which is the point of it, and also why it
  grows past the part anybody wants to read. Clearing it puts the whole thing
  in an `archive/` file first and tells you where. What it cannot clear is the
  container's own buffer: those lines come back on the next refresh, and the
  dialog says so rather than leaving you to think the button is broken.
- **Host and port can be copied separately.** The connection details for an
  application's database offered one field, labelled "Host", holding
  `host.docker.internal:3307` - the two glued together. Pasted into a
  `MYSQL_HOST` that has a `MYSQL_PORT` beside it, that is a port inside a
  hostname and a connection that never opens. All three shapes are offered
  now, each labelled for what it is.

### Fixed

- **The database host's port field says what it does not do.** It records
  where VibeSSH should connect - it does not move the server. Entering a port
  MariaDB is not listening on quietly broke every application that reached it,
  with nothing said anywhere. There is now a note under the field, including
  the part people find out the hard way: changing the server's real port needs
  **Fix container access** afterwards, because the firewall rule that lets
  containers in has the port written into it.
- **`PMA_HOST` and `PMA_PORT` explain themselves.** They are filled in once,
  when phpMyAdmin is created, from the database picked in the wizard - and the
  port is the one that database listens on *inside its own container*, not the
  one it publishes. Somebody published theirs on a different port, expected
  these to follow, and recreated the container when they did not. They were
  right to ask; nothing said either way.

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
