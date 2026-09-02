---
id: backups
title: Backups
section: applications
route: /applications
order: 50
---

A backup is a packed copy of an application's working directory - the Minecraft world, the configuration, the plugins. It is not a container image and not a snapshot of the whole Node; a container can be recreated at any time, the data inside it cannot.

## Where it is

Application -> the **Backups** tab. The list of backups, a **Back up now** button, and the **Automatic backups** card below it.

![The backup list: scheduled and manual, with sizes and the S3 upload marker](images/backups-tab.png)

## A manual backup

One click, one archive. In the list a backup is marked **Manual** or **Scheduled**, with its date and size.

You can back up a running application, but it is worth knowing what that means: files are copied while a process is writing to them. For a Minecraft server the usual practice is `save-all` and `save-off` in the console before, and `save-on` after. The most trustworthy backup is one taken while the application is stopped.

## Automatic backups

| Setting | What it does |
| --- | --- |
| Back up automatically | Turns the schedule on for this application. |
| Every N hours | The interval between backups. |
| Keep the last N | This many newest archives are always kept. |
| Max age in days | Older ones are removed. Empty means no limit. |
| Max total size in MB | The oldest are removed until the total is under it. Empty means no limit. |

The limits work together: **a backup is removed when it exceeds any one of them**. "Keep 10" together with "max 7 days" means a backup older than a week is deleted even while it is among the ten newest.

> The schedule only runs while VibeSSH is open. There is no background process and no daemon on the Node - if the desktop app is closed, a backup due at that time does not happen. Overdue backups are taken when it is opened again.

## Restoring

Restoring **overwrites the current files** in the working directory and cannot be undone. That is why the button is disabled while the application is running - restoring files underneath a live process produces a state that neither the backup nor what was there before ever had.

So the order is: stop the application, restore, start it.

## Downloading

Fetches the archive to this computer. Useful when you want a copy off the Node, or want to move the data somewhere else.

## External storage (S3)

A backup marked **Uploaded to external storage** was additionally copied to a configured S3-compatible store. The destination is configured in the application's settings, not here.

A backup that exists only on the same Node as the application protects you from your own mistake, but not from losing the Node. If the data has to survive the server, it needs a copy off it.

## Common mistakes

- **The schedule is set but there are no backups** - check whether VibeSSH was open at the time. It is the only thing that runs the schedule.
- **Backups are larger than expected** - a Minecraft world grows as it is explored. A total-size limit controls that better than a count.
- **I restored and got something other than I expected** - the date in the list is when the backup was taken, not when the server last saved the world.
