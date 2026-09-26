---
id: schedules
title: Schedules
section: applications
route: /applications
order: 55
---

A schedule restarts, stops or starts an application automatically at a set time - for example, restarting a server every day at 4:00.

## How to add a schedule

1. Open the application and go to the **Schedules** tab.
2. Click **Add schedule**.
3. Enter a **Name**, for example `Nightly restart`.
4. Under **What to do**, pick **Restart**, **Stop** or **Start**.
5. Under **When**, pick one of:
   - **Every day** and a time, for example `04:00`,
   - **On chosen days** and tick the days of the week,
   - **Every few hours** and choose how often,
   - **Custom (cron)**, if you know cron syntax.
6. Check the **First run** date under the form.
7. Click **Save**.

## How to check it works

1. Next to the schedule, click the **Run now** icon.
2. The application does that action straight away, exactly as it will at the scheduled time.
3. **Last run:** with the date appears under the schedule.

Each schedule also shows when it runs **next**.

## How to pause or delete a schedule

- **Pause:** turn off the switch next to the schedule. It stays on the list but does not run. Turn it back on to resume.
- **Delete:** click the bin icon and confirm.
- **Change:** click the edit icon, adjust it and save.

## Worth knowing

- **Schedules work without VibeSSH.** The Node runs them itself, so a 4:00 restart happens even when this computer is off. Automatic backups are different: they only run while VibeSSH is open.
- **Times follow the Node's clock.** If the Node is in a different time zone from you, your own time is shown next to it.
- **The server gets time to save.** On a restart or stop the server has up to 2 minutes to save its world before it is shut down by force.

## Common problems

- **There is no Schedules tab** - it is only there for Docker applications on a Node connected over SSH. Applications running on this computer and Nodes with the Vibe Agent do not have it.
- **"This Node has no cron"** - click **Install cron** under the message. VibeSSH installs it and saves the schedule straight away. You can also do it yourself in the Node's terminal: `sudo apt install cron`.
- **"Last run failed"** - hover over it to see why. Usually the application had been removed or Docker was not running.
- **It restarts at the wrong time** - check the Node's time zone, shown under the list.

## More detail

A schedule is written to the Node as a cron file and carried out by a small VibeSSH script. Deleting an application also removes its schedules from the Node. Migrating an application to another Node takes its schedules with it.
