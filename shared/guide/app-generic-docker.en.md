---
id: app-generic-docker
title: Docker container
section: blueprints
route: /applications
order: 150
---

Runs any Docker image. This is the escape hatch for everything that has no application type of its own in VibeSSH - nginx, WordPress, Grafana, whatever you find on Docker Hub.

It is also the type given to servers **adopted** by the discovery feature: VibeSSH replaces nothing in the directory, it just runs what is already there.

## Step by step

1. **Applications - New application**, choose the location and a name.
2. Pick **Docker container (generic)** from the list of types.
3. **Image** - the full name with a tag, e.g. `nginx:latest` or `itzg/minecraft-server:latest`.
4. **Command override** - leave it empty so the image runs as its author wrote it. Fill it in only to replace the image's own startup; one argument per line.
5. Create the application and press **Start**.

## Environment variables

Most images are configured with them. Their names are on the image's page on Docker Hub. You enter them under **Settings - Environment**; mark passwords and keys as **Secret**.

## Ports

Nothing is published automatically. Check the image's page for the port it listens on (nginx - `80`, phpMyAdmin - `80`, Grafana - `3000`) and add a port on the **Ports** tab: the internal one is the one from the image's documentation, the external one is your choice.

## Data and the working directory

The application's working directory is mounted into the container. Anything the program writes there survives a restart and a container recreate. Anything it writes elsewhere inside the container is lost on **Recreate container**.

**Where that directory appears inside the container** depends on where the application runs:

- **On a server** - at the same path it has on the server, for instance `/home/container/myapp`.
- **On your own computer** - at `/home/container`. The directory on disk stays where it is
  (`C:\Users\...`), but a Linux container cannot have a path with a drive letter in it, so
  inside it appears under a Linux path.

This only matters if you write absolute paths yourself, in the command or in environment
variables. File names relative to the working directory behave identically either way -
which is why the built-in application types use them.

## Docker on this computer

Choosing **This computer** with the **Docker container** runtime needs Docker Desktop
installed (on Windows, together with WSL2). VibeSSH checks while you are creating the
application and says so if it cannot see one.

A few things work differently from a server, deliberately:

- **There is no separate system account for the application.** That isolation is built
  from POSIX users and `chown`, which a local Docker Desktop does not have. Nothing is
  lost by it: the application's files are simply your own.
- **The console works differently underneath**, though it looks the same on screen.

**Fixed in 0.1.0-beta.17.** In earlier versions a local Docker application could be
created and then refused to start, with `internal error: DockerRuntime requires a
connection` - a message about SSH, despite "This computer" having been chosen. It affected
every Java application type (Paper, Velocity, Waterfall and the rest), because each of them
asks for that separate system account which does not exist locally. Starting, restarting
and **Recreate container** now work locally. If you still see that message, update the app.

## Switching to a managed type

If it turns out the container is really an ordinary Paper server, you can switch the application to the **Paper** type under **Settings - Application type**. From then on VibeSSH manages the server version - and it will download its own server file into that directory. The warning shown when switching says exactly what will happen.

## Common problems

**`no such image` or a pull error.** A typo in the image name, or the image is private - registry credentials are added in the application's **Settings**.

**The container starts and stops.** Look at **Logs**. Usually a required environment variable from the image's documentation is missing.
