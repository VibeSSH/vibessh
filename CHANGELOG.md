# Changelog

What changed in each release, written for the person using VibeSSH rather
than for the person who wrote it. The commit history has the reasoning.

This file starts at 0.1.0-beta.5. Earlier releases were published without one;
their contents are in the git history between the tags.

## 0.1.0-beta.17

### New

- **Closing the window leaves VibeSSH beside the clock instead of quitting.**
  The close button used to end the program, and with it every live SSH
  session, port forward and followed log - which is not what most people mean
  when they close a window to get it off the screen. It now hides the window
  and leaves a tray icon: click it to bring the window back, or right-click
  for a menu with **Show VibeSSH**, **Check for updates...** and **Quit
  VibeSSH**. The first time this happens you get a notification saying where
  the window went, because a program still running after you closed it, with
  no word at all, is indistinguishable from one that failed to quit.

  If you would rather the close button quit, turn off **Keep VibeSSH running
  in the tray** under **Settings - Preferences**. The setting is saved the
  moment you change it.

  On Windows the tray icon is often tucked under the **^** arrow beside the
  clock - click it to expand the list. The guide page for Settings covers
  this, including what to do when you have closed the window and cannot find
  the program.

## 0.1.0-beta.16

### Fixed

- **A Docker application on your own computer can now be created at all.**
  Choosing the Docker runtime for a local application - a Paper server, say -
  produced a container the daemon refused to make. The working directory
  VibeSSH suggests on Windows is a Windows path, and that same path was handed
  to the container as its own; a Linux container has no
  `C:\Users\...\applications\paper`, so Docker complained about a path, and
  there was nowhere in the app to give it a different one. The directory is
  now mounted at `/home/container` inside the container, which is where a
  remote application's files already live. Applications on a server are
  unaffected - their working directories were always paths a container could
  have, and they keep them.
- **The "Create application" wizard says what it is waiting for, instead of
  only greying out "Next".** Each step now names the answers still missing, by
  the label they carry on screen. Two ways of getting stuck had nothing to
  read at all: a working directory that could not be suggested, which left the
  field empty with no explanation, and a required setting on the configuration
  step that had scrolled out of sight. When VibeSSH cannot work out where to
  keep an application's files on this computer, it now says so and says why,
  rather than leaving you to invent a path.

## 0.1.0-beta.15

### Changed

- **The update check asks VibeSSH's own server instead of GitHub's.** It sends
  nothing it was not already sending - a version, a platform, and the address
  every HTTP request carries - and GitHub stays configured as a fallback, so
  an update still happens if that server is unreachable. What it changes is
  who can see how many installations are running: previously only GitHub
  could, and only as a download count that cannot tell one machine left open
  from forty opened once.

  What is kept from it is a day, a version, a platform, and a hash of the
  address salted with a value that changes daily - so the same machine is one
  entry within a day and cannot be joined to the day before, by us either.
  The terms page says so, and the site's security section no longer claims
  nothing leaves your computer without an account, which this check has
  always contradicted.

## 0.1.0-beta.14

### New

- **A server shared with your team can be added to your own list in one
  click.** Being given an account on somebody's machine was not the same as
  being able to reach it: the account was created, and nothing told you its
  name, so a member saw a server in the team, no way to connect, and no
  explanation. Each shared server you have not added yet now says what you
  sign in as and which key you use, and offers to add it - with your account
  and your own key filled in. If nobody has synced access to that server
  yet, it says that instead, rather than letting you find out from a refused
  login.

## 0.1.0-beta.13

### Fixed

- **Your own machine registers itself, not only the moment you sign in.** The
  key that lets a teammate's install create your account on a shared server
  was published from the sign-in screen alone, so an installation that was
  already signed in never published at all. Syncing access then reported you
  as somebody who "has not opened VibeSSH on any device" - while you had it
  open in front of you - and your own account was the one that never got
  created. It is now registered on every launch as well.
- **A shared server keeps its name on screen after a sync.** The results
  appeared as a full-width block on a row that does not wrap, which squeezed
  the server's name and address out of existence - so with more than one
  server there was no way to tell which one the results belonged to.
- **Uploads and downloads stop failing after a while with "handle limit
  reached".** File transfers left their remote file to be tidied up on the way
  out instead of closing it and waiting for the server to say so. The server
  did free it; the count kept on this side did not, and once that count
  reached the server's ceiling every further transfer was refused before a
  single byte went out - with a message that reads like the server's fault and
  is not. Every transfer now closes what it opened, including the ones that
  fail halfway, and including a download that cannot write its local file.

  The cause is a bug in the SFTP library rather than in how VibeSSH uses it,
  and it is reported upstream:
  [AspectUnk/russh-sftp#98](https://github.com/AspectUnk/russh-sftp/issues/98).

### Security

- **rustls updated to 0.23.45**, which closes
  [RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285): TLS
  1.3 handshake messages were accepted at the wrong encryption level when
  they followed a key change in the same record. The transcript is still
  authenticated, so this could not be used to alter or complete a handshake -
  the effect is that a peer could send in plaintext what should have been
  encrypted, and rustls did not refuse the connection.

## 0.1.0-beta.12

### New

- **A teammate can be given their own account on your server.** Add them to a
  team, press **Sync access** on the server, and VibeSSH creates a Linux
  account for them on that machine and installs the key their own copy of the
  app published. Nothing secret passes between you - only public keys, the
  same kind of line every `authorized_keys` file in the world holds in plain
  text. The server's log then names a person rather than "whoever had the
  key", and taking one person's access away no longer means rotating a key
  and handing the new one to everybody who stays.
- **What that account may do comes from their role.** A role that grants
  nothing privileged leaves an account that cannot use `sudo` on that machine
  at all - the first time "view only" means something a person cannot talk
  their way around rather than a button the app declines to show. Viewing and
  starting or stopping applications become a short list of allowed commands,
  scoped to VibeSSH's own containers. Some permissions cannot be narrowed and
  the app says so instead of pretending: a terminal, installing packages or
  creating applications are root on that machine whatever the role is called.
  A role change reaches the server at the next access sync.
- **Removing somebody from a team now reaches the machines they were on.**
  It used to change only the team list, leaving their account, their key and
  their `sudo` rule exactly where they were while the screen reported the
  removal as done. What is still owed is now recorded and shown as
  **pending**, naming the person, the account and which server still has it,
  until an installation that can reach that machine carries it out. Access
  removed on a machine nobody can reach is now visible rather than silent.
- **Applications a team can see.** Share an application and everybody in the
  team sees it with its ports and settings. Values marked secret stay on the
  server - the team sees that a secret is set, never what it is.
- **PostgreSQL.** A database server of its own, in the same shape as MariaDB
  and MongoDB, with a built-in template, a `psql` console for asking it
  questions, and a step-by-step page in the guide. The template fills in the
  data directory, which is the setting that otherwise loses the whole
  database the first time the container is recreated.

### Fixed

- **A port is no longer called protected when it is not.** The Ports tab read
  a badge from what the firewall was *meant* to say; it now reads the
  machine's own rules, and says **unknown** when it cannot read them rather
  than guessing in the reassuring direction.
- **Passwords and tokens stop travelling where anyone on the server can see
  them.** Secrets no longer appear in command arguments, which every account
  on the machine can list, and service files that carry them are no longer
  world-readable. Copy and restore operations stay inside the application's
  own directory, and archives written while working are private to it.
- **Repeated sign-in attempts are limited.** Ten failures from one address,
  then a refusal, rather than an unlimited supply of guesses.
- **Vibe AI says what actually went wrong.** "The backend has no model
  configured" and "the provider returned nothing" used to arrive as "try
  again later", which is advice that could not work.
- **The guide shows the app in the language you are reading it in.** The
  English pages had Polish screenshots.
- **The handshake claim in the Vibe Network guide was too strong.** A
  handshake is evidence the tunnel works, not proof, and the page now says
  which parts it does not cover.

## 0.1.0-beta.11

### New

- **There is a hosted account backend, and the app points at it by default.**
  Teams, roles and the shared parts of VibeSSH no longer need anything set up
  by hand.
- **Refusals from the account backend arrive in your own language.** A Polish
  interface showed English sentences from the server inside Polish ones -
  "Brak uprawnien: invalid email or password" on the sign-in screen, among
  others.

## 0.1.0-beta.10

### New

- **Every port says whether it is actually protected.** A published Docker
  port is bound widely and only the node's firewall narrows it, so a port
  marked "Vibe Network only" that nothing is enforcing is open to the
  internet. The app knew this and would tell you - as a summary, after you
  pressed Sync Firewall, once for the whole tab. Each port now carries its own
  **Protected** or **Unprotected** badge, read when the tab opens. A public
  port gets no badge at all, because it is meant to be reachable and a red
  mark on every correct port is how people learn to ignore red marks.
- **The arrow keys walk back through commands already sent**, in an
  application's console. Up for older, down for newer, and down past the
  newest gives back whatever you had half-typed. Remembered per application,
  so it survives a trip to the Logs tab and back.
- **A walkthrough from beginning to end**: two servers, a private network
  between them, and a database only they can reach. Seven steps, each with a
  screenshot and what you should see before moving on, and a table for when
  half of it works - the network up but the firewall not, or the other way
  round.
- **A glossary** of the words the interface uses without explaining them:
  endpoint, peer, bind address, reconcile, handshake, source CIDR, and the
  difference between an internal and an external port.

### Fixed

- **The Security page no longer contradicts the Agent page.** One said the
  public agent release did not exist yet; the other gave a working install
  command. The agent is released, and what that page says now is the thing it
  was reaching for: the installer checks a checksum fetched from the same host
  as the binary, which catches a corrupted download and not a compromised
  release host.

### Under the hood

- The repository is a set of modules rather than a frontend with some other
  things beside it: `apps/desktop/{ui,src-tauri}`, `apps/agent`,
  `apps/backend`, `crates/protocol`, and `shared/guide` for the corpus that is
  compiled into both halves. Contributors should read
  `docs/repository-structure.md` before moving anything - it lists the
  compile-time couplings that a rename breaks.

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
