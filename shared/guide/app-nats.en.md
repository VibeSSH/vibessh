---
id: app-nats
title: NATS
section: blueprints
route: /applications
order: 240
---

NATS is a message broker. Applications publish messages to it and subscribe to them, instead of connecting to each other directly.

It is useful when several services have to talk: a server publishes an event, a bot receives it, a panel displays it. For a single application it is not needed.

## Step by step

1. **Applications - New application**, choose the location and a name, e.g. `nats`.
2. Pick **NATS** from the list of types.
3. **NATS version** - leave `2`.
4. **Enable JetStream (persistence)** - tick it if messages have to survive a broker restart. Without it NATS only forwards: whoever is not listening at that moment does not get the message.
5. **Auth token** - set one if the port will be published.
6. Create the application and press **Start**.

## Connecting to it

The broker listens on port **4222** inside its container.

1. Open the client application, go to the **Ports** tab, find the **Connections** card, connect it to the NATS application.
2. In the client, use the address `nats://APPLICATION-NAME:4222`, with the name in lower case.
3. If you set a token, pass it the way your client library documents.

## Common problems

**The client will not connect.** Check the **Connections** card and the address - it has to be the application's name, not `localhost`.

**Messages are lost while the receiver is offline.** That is NATS without JetStream. Enable **JetStream** in the configuration and use a stream on the client side.

**`authorization violation`.** The broker has a token and the client is sending none, or a different one.
