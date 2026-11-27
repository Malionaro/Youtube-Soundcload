#!/bin/sh
# ─────────────────────────────────────────────────────────────────────────────
# soundsync-relay Installer (Raspberry Pi / Linux-Server, amd64 + arm64)
#
#   curl -fsSL <diese-Datei> | sh
#   oder:  sh install.sh
#
# Server hier: 65.21.135.40  ->  https://65.21.135.40
#
# Env (optional):
#   SERVER_IP=65.21.135.40
#   ADVERTISED=https://65.21.135.40      # Adresse im Einladungs-Link
#   SOUNDSYNC_RELAY_TOKEN=...            # sonst wird ein Token erzeugt
#   SOUNDSYNC_TEAM_ID=team
#   PORT=3030
#   WITH_CADDY=0|1                        # Reverse-Proxy/TLS aufsetzen (Caddy)
#   CADDY_ADDR=65.21.135.40               # Domain oder IP für Caddy
# ─────────────────────────────────────────────────────────────────────────────
set -eu

SERVER_IP="${SERVER_IP:-65.21.135.40}"
ADVERTISED="${ADVERTISED:-https://65.21.135.40}"
TEAM_ID="${SOUNDSYNC_TEAM_ID:-team}"
PORT="${PORT:-3030}"
RELAY_DIR="${SOUNDSYNC_RELAY_DIR:-/var/lib/soundsync-relay}"
WITH_CADDY="${WITH_CADDY:-0}"
CADDY_ADDR="${CADDY_ADDR:-$SERVER_IP}"
INSTALL_PREFIX="${INSTALL_PREFIX:-/usr/local/bin}"
SERVICE_NAME="soundsync-relay"
SRC_DIR="${SRC_DIR:-}"

say() { printf '\n==> %s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

# ── root? ────────────────────────────────────────────────────────────────────
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if have sudo; then SUDO="sudo"; else echo "Bitte als root starten."; exit 1; fi
fi

say "Server: $SERVER_IP   Advertised: $ADVERTISED   Port: $PORT"

# ── Abhängigkeiten ───────────────────────────────────────────────────────────
if have apt-get; then
  $SUDO apt-get update -y || true
  $SUDO apt-get install -y curl ca-certificates git build-essential || true
elif have dnf; then
  $SUDO dnf install -y curl ca-certificates git gcc make
elif have pacman; then
  $SUDO pacman -Sy --noconfirm curl ca-certificates git base-devel
fi

# Rust
if ! have cargo; then
  say "Rust installieren…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  # shellcheck disable=SC1090
  . "$HOME/.cargo/env"
fi

# ── Quelle ───────────────────────────────────────────────────────────────────
if [ -z "$SRC_DIR" ]; then
  if [ -f Cargo.toml ] && [ -d src ]; then
    SRC_DIR="$PWD"
  elif [ -f src-tauri/Cargo.toml ]; then
    SRC_DIR="$PWD/src-tauri"
  else
    say "SoundSync klonen…"
    REPO="${REPO:-https://github.com/Malionaro/Youtube-Soundcload}"
    tmp="$(mktemp -d)"
    git clone --depth 1 "$REPO" "$tmp/repo"
    SRC_DIR="$tmp/repo/soundsync-tauri/src-tauri"
  fi
fi

say "Bauen: soundsync-relay…"
( cd "$SRC_DIR" && cargo build --release --bin soundsync-relay )
BIN="$SRC_DIR/target/release/soundsync-relay"
# auch CARGO_TARGET_DIR beachten
[ -f "$BIN" ] || BIN="${CARGO_TARGET_DIR:-$SRC_DIR/target}/release/soundsync-relay"

$SUDO install -m 0755 "$BIN" "$INSTALL_PREFIX/soundsync-relay"
say "Installiert: $INSTALL_PREFIX/soundsync-relay"

# ── Token ────────────────────────────────────────────────────────────────────
TOKEN="${SOUNDSYNC_RELAY_TOKEN:-}"
if [ -z "$TOKEN" ]; then
  if have openssl; then TOKEN="$(openssl rand -hex 24)"; else TOKEN="$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"; fi
fi

$SUDO mkdir -p "$RELAY_DIR"
$SUDO chown -R "${SUDO_USER:-$(id -un)}" "$RELAY_DIR" 2>/dev/null || true

# ── systemd ──────────────────────────────────────────────────────────────────
if have systemctl; then
  say "systemd-Service anlegen…"
  $SUDO tee /etc/systemd/system/$SERVICE_NAME.service >/dev/null <<EOF
[Unit]
Description=SoundSync Team-Mode Relay / Coordinator
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=$INSTALL_PREFIX/soundsync-relay
Restart=always
RestartSec=3
Environment=PORT=$PORT
Environment=SOUNDSYNC_RELAY_TOKEN=$TOKEN
Environment=SOUNDSYNC_TEAM_ID=$TEAM_ID
Environment=SOUNDSYNC_RELAY_DIR=$RELAY_DIR
Environment=SOUNDSYNC_ADVERTISED_ADDR=$ADVERTISED

[Install]
WantedBy=multi-user.target
EOF
  $SUDO systemctl daemon-reload
  $SUDO systemctl enable --now "$SERVICE_NAME"
  sleep 1
else
  say "Kein systemd – manuell starten:"
  echo "  SOUNDSYNC_RELAY_TOKEN=$TOKEN SOUNDSYNC_ADVERTISED_ADDR=$ADVERTISED PORT=$PORT $INSTALL_PREFIX/soundsync-relay"
fi

# ── Firewall (ufw/firewalld best effort) ─────────────────────────────────────
if have ufw; then $SUDO ufw allow "$PORT/tcp" || true; fi
if have firewall-cmd; then $SUDO firewall-cmd --permanent --add-port="$PORT/tcp" && $SUDO firewall-cmd --reload || true; fi

# ── Reverse-Proxy + TLS (Caddy) für https:// ────────────────────────────────
if [ "$WITH_CADDY" = "1" ]; then
  say "Caddy Reverse-Proxy aufsetzen…"
  if have apt-get; then
    curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key | $SUDO tee /usr/share/keyrings/caddy-stable-archive-keyring.gpg >/dev/null
    curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt | $SUDO tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null
    $SUDO apt-get update -y || true; $SUDO apt-get install -y caddy
  fi
  $SUDO tee /etc/caddy/Caddyfile >/dev/null <<EOF
$CADDY_ADDR {
    reverse_proxy localhost:$PORT
    # Bare-IP / selbstsigniert? dann in der App http statt https nutzen.
    # tls internal
}
EOF
  $SUDO systemctl reload caddy || $SUDO systemctl restart caddy || true
  if have ufw; then $SUDO ufw allow 443/tcp || true; $SUDO ufw allow 80/tcp || true; fi
fi

# ── Fertig ───────────────────────────────────────────────────────────────────
LINK="soundsync-team://${ADVERTISED#http://}${ADVERTISED#https://}/$TEAM_ID?token=$TOKEN"
# sauberer Link
CLEAN_ADDR="$ADVERTISED"
LINK="soundsync-team://${CLEAN_ADDR}//$TEAM_ID?token=$TOKEN"
LINK="$(printf '%s' "soundsync-team://X/$TEAM_ID?token=$TOKEN" | sed "s|soundsync-team://X|soundsync-team://$CLEAN_ADDR|")"

say "FERTIG."
echo
echo "Server-gehostete API:   $ADVERTISED   (Relay lauscht lokal auf :$PORT)"
echo "Health:                 curl ${ADVERTISED%/}/health"
echo "Token:                  $TOKEN"
echo "Team-ID:                $TEAM_ID"
echo
echo "WICHTIG zu HTTPS:"
echo "  * Mein Relay spricht HTTP auf :$PORT. Für '$ADVERTISED' (https) braucht ein"
echo "    Reverse-Proxy TLS (z.B. WITH_CADDY=1 + gültiges Zertifikat / Domain)."
echo "  * Kein Port-Forwarding auf den PCs nötig – die verbinden sich nur AUSGEHEND."
echo "  * Nur dieser Server ($SERVER_IP) muss erreichbar sein."
echo
echo "In SoundSync auf BEIDEN PCs: Team-Download (P2P) -> 'Mit Server verbinden'"
echo "  Server-URL: $ADVERTISED"
echo "  Token:      $TOKEN"
echo
echo "Logs: journalctl -u $SERVICE_NAME -f"
