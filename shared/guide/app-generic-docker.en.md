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

## Switching to a managed type

If it turns out the container is really an ordinary Paper server, you can switch the application to the **Paper** type under **Settings - Application type**. From then on VibeSSH manages the server version - and it will download its own server file into that directory. The warning shown when switching says exactly what will happen.

## Common problems

**`no such image` or a pull error.** A typo in the image name, or the image is private - registry credentials are added in the application's **Settings**.

**The container starts and stops.** Look at **Logs**. Usually a required environment variable from the image's documentation is missing.
