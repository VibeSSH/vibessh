---
id: application-files
title: Application files
section: files
route: /applications
order: 20
---

The file browser shows an application's working directory, and only that. It is not a file manager for the whole server; there is no way out of the application's directory from this tab, including by typing a path.

## Where it is

Application -> the **Files** tab.

## Which account this runs as

If an application has a dedicated account, VibeSSH creates a separate system account on the Node (`vibessh-app-...`) and performs every file operation as it. This is not cosmetic: without it, every application on a Node could read every other application's files.

The account and its helper script are set up on the first file operation, so the first visit to the tab can be slightly slower than the ones after it.

![The application file browser, with directories and configuration files](images/application-files.png)

## Getting around

Clicking a directory enters it; the breadcrumbs above the list lead back. A directory you have already visited is remembered, so walking back up the tree is instant - VibeSSH asks the Node whether anything changed a moment later.

The list shows at most 200 entries. The **Filter** box narrows it to what you are looking for - a directory with thousands of files is there to be filtered, not scrolled.

## The editor

Clicking a file opens it in an editor with syntax highlighting. `.yml`, `.json`, `.properties`, `.toml`, `.sh`, `Dockerfile` and systemd unit files are among those recognised.

- **Search** (the magnifier, or `Ctrl+F`) - find in file, with matches highlighted.
- **Ctrl+S** - saves, exactly as the button does.
- **History** - previous versions of the file, as saved by this editor.
- **Back up before saving** - ticked by default. Before overwriting a file its current contents go into the history. Turn it off deliberately.

Files over 1 MB do not open in the editor. The limit is enforced by the backend, not only by the interface.

### YAML validation

`.yml` and `.yaml` files are checked for syntax as you type. A line with an error gets a tinted background, a bar down its left edge, and a dot in the gutter.

**A syntax error blocks saving.** That is deliberate: a configuration file that does not parse stops the server from starting, and the failure surfaces a minute later, somewhere else, with no sign of the cause.

Only **errors** block. Warnings do not - a warning is the parser saying something is unusual, and refusing to save over that would be the editor overruling you.

A duplicate key blocks too. That is the case where the second one silently wins, so a setting has a different value from the one you can see in the file.

## File operations

Right-click an entry: rename, move, copy, permissions, delete, and for `.zip` archives, extract.

Uploads and downloads go through the transfer queue at the bottom. A transfer that fails can be retried without picking the file again.

## Common mistakes

- **I can't see a file I uploaded over FTP** - refresh the listing. A remembered directory shows the last read until VibeSSH asks again.
- **Saving does nothing and I don't know why** - if it is YAML, look at the red bar above the editor. It names the line and the reason.
- **I changed a file and the server still behaves the old way** - most servers read their configuration at start. After saving, restart the application.
