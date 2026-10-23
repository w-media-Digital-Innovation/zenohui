# zenohui v0.2.0

Configure your Zenoh connections directly in the terminal, and see remote routers alongside your local network.

- **Settings tab:** press **4** to edit session mode, connection endpoints, listeners and multicast discovery.
- **Apply without leaving the UI:** **F5** connects in the background. Failed attempts preserve your current connection and history. Successful switches start a fresh history.
- **Connection profiles:** **F2** saves and **F3** loads `zenohui-profile.json` in the working directory. Profiles are loaded explicitly and contain endpoint strings in plain text.
- **Remote routers:** the Routers and Peers tabs combine multicast discovery with current session IDs. A Status column distinguishes connected nodes from discovered nodes.
- **Mouse navigation:** click the four tab headers and select router/peer rows.
- Full locator details and compact address summaries remain available.

## Try it

```bash
zenohui --peer tcp/YOUR_ROUTER:7447
```

To connect only through a router:

```bash
zenohui --mode client --peer tcp/YOUR_ROUTER:7447 --no-multicast
```

Use your router's configured port. Explicit listener addresses already used by the current session may require restarting zenohui. Remote node locators may be unavailable through session information, even when the node is connected.

Rust **1.92.0 or newer** is required to build from source. Download the archive or package matching your operating system and architecture below.
