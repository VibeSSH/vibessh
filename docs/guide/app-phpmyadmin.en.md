---
id: app-phpmyadmin
title: phpMyAdmin
section: blueprints
route: /applications
order: 220
---

phpMyAdmin is a web page for browsing and editing the contents of a MySQL/MariaDB database - tables, rows, SQL queries, import and export.

It stores nothing itself. It is only an interface to a database you have to point it at, and that pointing is the one thing here that has to be right.

## Step by step

1. **Applications - New application**.
2. Under **Templates**, pick **phpMyAdmin for a MariaDB application**.
3. Choose the location - **the same one the database is on**. A phpMyAdmin on your own computer cannot reach a database on a Node, or the other way round.
4. Give it a name, e.g. `phpmyadmin`.
5. In the settings step there is a **Database** field. Pick the one it should manage. The list holds MariaDB applications and database hosts from that location.
6. Create the application and press **Start**.

The **Database** field does three things at once that you previously had to guess at: it fills in `PMA_HOST`, it fills in `PMA_PORT`, and - for a MariaDB application - it grants the connection between the two containers. Without that third step no address works at all, because containers cannot see each other.

## How to open the panel

phpMyAdmin listens on port **80** inside its container. To reach it with a browser, that port has to be published:

1. Open the application, go to the **Ports** tab, press **Add port**.
2. **Internal port**: `80`. **External port**: any free one, e.g. `8080`.
3. **Network access**: choose **Vibe Network** if you possibly can.
4. Press **Sync firewall**.
5. Open `http://NODE-ADDRESS:8080`.

Do not use port **443** unless you have a certificate. phpMyAdmin will decide it is running over HTTPS, set a secure session cookie, the browser will refuse it, and you will see *Failed to set session cookie*.

Think twice before choosing **Public**. This is a database administration panel, protected by nothing but the database password.

## How to log in

With the username and password **of the database**, not of VibeSSH.

- A database from a MariaDB application: `root` with `MYSQL_ROOT_PASSWORD`, or the account from `MYSQL_USER` / `MYSQL_PASSWORD`.
- A database from the **Databases** tab: press the eye icon next to it to see the generated username and password.

## Common problems

**"getaddrinfo for db failed", or another name that does not exist.** `PMA_HOST` points at a host that is not there - usually `db`, copied from a docker-compose tutorial. Fix it under **Settings - Environment**: for a MariaDB application it is that application's name in lower case; for a database host it is the address shown on the **Databases** tab after the `@`.

**Login hangs and ends in a timeout.** The credentials are fine but the packet never arrives. If the database is a database host on a Node, open **Databases** and press the refresh icon next to it - **Fix container access**. That binds the database server to the Docker bridge as well as loopback, and adds a firewall rule scoped to that bridge. If the database is a MariaDB application, check the **Connections** card instead.

**"Failed to set session cookie".** See above - port 443 without HTTPS, or `PMA_ABSOLUTE_URI` set to an address other than the one you use. If you have that variable, put exactly the address from your browser's address bar in it, with the scheme, the port and the trailing slash.

**You want to type the database address at login instead.** Use the **phpMyAdmin for any server** template. It sets `PMA_ARBITRARY=1`, which adds a server field to the login page. Useful for databases outside VibeSSH.
