---
id: app-python-bot
title: Python bot
section: blueprints
route: /applications
order: 260
---

Runs your own Python program - a bot, a script, a small API. VibeSSH shows its logs, restarts it, and keeps it running.

## Step by step

1. **Applications - New application**, choose the location and a name, e.g. `bot`.
2. Pick **Python bot** from the list of types.
3. **Entry file** - the path relative to the working directory, e.g. `bot.py` or `src/main.py`.
4. **Python version** - leave `3.13` unless your code needs an older one.
5. **Program arguments** - usually empty.
6. Create the application, but **do not start it yet**.
7. **Files** tab - upload the code along with `requirements.txt`.
8. **Settings - Environment** - add the token or API key and tick **Secret**.
9. Press **Start**.

## Dependencies from requirements.txt

This application runs `python YOUR-FILE` and does not run `pip install` by itself. Install the libraries from the **Actions** tab, or in the terminal, in the application's working directory.

## Updating the code

Upload the new files on the **Files** tab and press **Restart**.

## Common problems

**`ModuleNotFoundError`.** The dependencies were never installed - see the section above.

**The program exits immediately with no error.** A script that simply ran to the end exits successfully. A bot is supposed to loop - check that you actually call `run()` or `asyncio.run(...)`.

**Accented characters look wrong in the logs.** Set the `PYTHONIOENCODING` variable to `utf-8`.
