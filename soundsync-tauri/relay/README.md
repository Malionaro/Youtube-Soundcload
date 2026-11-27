# Free relay for Team-Mode (Raspberry Pi or any always-on server)

The SoundSync **Team-Mode** is **local-only by default**: the second PC has to reach
the first PC's `http://<host-ip>:3030`. The good news: the protocol is
**member-dials-host only** – the member sends `claim` / `done` / `fail` / `state` /
`file` requests *to* the host; **the host never needs to reach the member.**

So **all you need is to expose the host PC's port `3030` through your Pi.** No
SoundSync changes on the server, no paid service.

Once reachable, create the team in SoundSync with **“reachable address”** set to
`<PI_IP>:<PUBLIC_PORT>` (or edit the generated `soundsync-team://…` link so the
address part points at the Pi). `team_id` + `token` stay the same.

Pick **one** of the options below. All are free.

---

## Option A — SSH reverse tunnel (nothing to install on the Pi)

Pi: enable SSH (`sudo systemctl enable --now ssh`) and allow inbound remote ports:

```sh
# /etc/ssh/sshd_config  on the Pi
GatewayPorts yes          # let the Pi listen on 0.0.0.0 for forwarded ports
# then: sudo systemctl restart ssh
```

Host PC (the one running SoundSync + the team): run `ssh-reverse-tunnel.sh`
(see `ssh-reverse-tunnel.sh`):

```sh
PI_IP=192.168.1.50 PI_PORT=13337 sh ssh-reverse-tunnel.sh
# => ssh -N -R 0.0.0.0:13337:localhost:3030 pi@192.168.1.50
```

Then **reachable address** in SoundSync = `<PI_IP>:13337`.

Only requirement: the **member** PC can reach `<PI_IP>:13337`. If both PCs are on
different networks, the Pi itself must have a reachable address (own LAN IP /
public IP / its own free VPN such as Tailscale).

---

## Option B — frp (free, no SSH key juggling)

`frp` = free open-source tunnel. Download `frp_*.zip` from
`https://github.com/fatedier/frp/releases` (arm64 build for Pi).

Pi (server): `frps -c frp/frps.toml`
Host PC (client): `frpc -c frp/frpc.toml`

Then **reachable address** = `<PI_IP>:13337`. See `frp/frps.toml`, `frp/frpc.toml`.

### docker compose (on the Pi)

```sh
docker compose -f relay/docker-compose.frp.yml up -d
# frpc still runs on the host PC (it must tunnel the host's local 3030)
```

---

## Option C — Tailscale (simplest, no relay you run)

Install **Tailscale** (free personal tier) on Pi + both PCs. Put all three in one
tailnet; then no tunnel is needed at all: the member reaches the host at its
**Tailscale IP `100.x.y.z:3030`**. You can even skip the Pi for connectivity (it's
then just a free always-on VPN node). SoundSync stays **local-only**, just over a
private network.

---

## What the app does

`team_create(..., advertised_addr)` (UI field **“reachable address”**) makes the
generated `soundsync-team://…` link point at `advertised_addr` instead of the
local `IP:3030`. `team_join` accepts any reachable `host:port`. **Nothing else in
SoundSync changes.**

---

## NEU: Server-gehostete API / eigener Relay (`soundsync-relay`)

Statt eines PCs als Host kann ein **eigener, immer erreichbarer Server/Pi** der **Koordinator ("Host")** sein. **Beide PCs verbinden sich nur AUSGEHEND** zum Relay. **Kein Port-Forwarding auf den PCs.** Kostenlos (dein eigener Server, keine externe Plattform).

Der Relay ist ein **eigenständiges kleines Binary** ohne Tauri und **ohne yt-dlp** – er koordiniert nur + sammelt fertige Dateien. Die PCs downloadet weiterhin lokal selbst (eigenes `yt-dlp`, eigene Cookies, eigene IP – was für YouTube-Sperre-Failover wertvoll ist).

### Bauen
```sh
cd soundsync-tauri
cargo build --release --bin soundsync-relay
# Binary: target/release/soundsync-relay
```

### Starten (z.B. Raspberry Pi)
```sh
SOUNDSYNC_RELAY_TOKEN=dein-langes-secret \
SOUNDSYNC_TEAM_ID=myteam \
SOUNDSYNC_RELAY_DIR=/var/lib/soundsync-relay \
SOUNDSYNC_ADVERTISED_ADDR=pi.example.com:3030 \
PORT=3030 soundsync-relay
```
Ausgabe u.a.:
```
soundsync-relay läuft auf 0.0.0.0:3030
Token: dein-langes-secret
Einladungs-Link (auf BEIDEN PCs einfügen -> Beitreten):
  soundsync-team://pi.example.com:3030/myteam?token=dein-langes-secret
```
Health: `GET http://<server>:3030/health` -> `{"ok":true}` (ohne Token).

### App anbinden
Auf **PC A und PC B**: SoundSync -> *Team-Download (P2P)* -> **Beitreten** mit dem ausgegebenen `soundsync-team://…`-Link (`team_join` mit der Relay-Adresse; `team_create` wird in diesem Modus nicht gebraucht). Dann je PC **eigene Playlist** hinzufügen, auf beiden **Team-Download starten**.

### systemd (Pi/Linux)
```sh
sudo cp soundsync-relay /usr/local/bin/
sudo cp relay/soundsync-relay.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now soundsync-relay
```

### Docker
```sh
docker build -f relay/Dockerfile.relay -t soundsync-relay .
docker run -d -p 3030:3030 -v relay-data:/data \
  -e SOUNDSYNC_RELAY_TOKEN=dein-langes-secret \
  -e SOUNDSYNC_ADVERTISED_ADDR=pi.example.com:3030 \
  soundsync-relay
```

### API (alle `/team/*`, außer `/health`, Token via `X-Team-Token` / `Authorization: Bearer` / `?token=`)
`GET /team/state`, `POST /team/join`, `POST /team/leave`, `POST /team/playlist`,
`POST /team/claim`, `POST /team/progress`, `POST /team/done`, `POST /team/fail`,
`POST /team/heartbeat`, `POST /team/file`, `GET /team/file/<name>`.

### Failover auf dem Relay
`coord_sweep` (alle 5 s) verteilt **hängende** Aufgaben neu, nimmt Aufgaben **offline-gegangener** PCs zurück und gibt sie frei; ein **YouTube-gesperrter** PC (`/team/fail blocked=true`) wird markiert, kriegt vorübergehend **keine** Aufgaben mehr, seine offenen Tracks gehen an den anderen PC, nach Cooldown reiht er sich wieder ein. Tracks mit zu vielen Versuchen -> `failed`.