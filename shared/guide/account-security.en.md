---
id: account-security
title: Two-step verification
section: getting-started
route: /settings
order: 155
---

Two-step verification (2FA) means signing in to your VibeSSH account takes your password and a code from your phone. Somebody who learns your password still can't get in without the phone.

## What you need

An authenticator app on your phone, such as **Google Authenticator**, **Aegis** or **1Password**.

## How to turn it on

1. Sign in to your VibeSSH account.
2. Go to **Settings** → the **Account and security** card.
3. Click **Turn on**, then **Continue**.
4. Scan the QR code with the app on your phone. If you can't, type the key shown under the code into the app.
5. Enter the 6-digit code the app shows and click **Turn on**.
6. Keep the **recovery codes** somewhere safe (a password manager, for example). Click **Copy codes**, tick **I've saved the recovery codes** and click **Done**.

The recovery codes are shown only once. Without them and without your phone, you can't get into the account.

## How to sign in

1. Enter your email and password as usual.
2. When the **Code from the app** field appears, enter the current code from your phone.

No phone? Click **Don't have your phone? Use a recovery code** and enter one of the codes you saved. Each recovery code works once.

## How to turn it off

1. **Settings** → **Account and security** → **Turn off**.
2. Enter your password and a code from the app (or a recovery code).
3. Click **Turn off**.

## Common problems

- **"That code isn't right"** - usually the phone's clock is fast or slow. Turn on automatic time in the phone's settings. A code also works only once, so after using one, wait for the next.
- **I lost my phone** - sign in with a recovery code, turn two-step verification off, and turn it on again with the new phone.
- **"Two-step verification isn't available on this account server"** - the account server has no encryption key configured. This only applies to a self-hosted account server, not api.vibessh.dev.
- **The phone app doesn't ask for a code** - the VibeSSH mobile app doesn't support codes yet. Until it is updated, sign in from your computer.

## More detail

The key your phone derives the codes from is stored encrypted on the account server. The QR code is drawn on your computer, so the key is never sent to an outside service.
