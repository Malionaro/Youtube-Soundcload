#!/bin/sh
# Expose the host PC's SoundSync port 3030 through your Raspberry Pi.
#
# Pi:  sudo systemctl enable --now ssh
#      /etc/ssh/sshd_config -> "GatewayPorts yes" ; sudo systemctl restart ssh
#
# Usage on the HOST PC:
#   PI_IP=192.168.1.50 PI_PORT=13337 sh ssh-reverse-tunnel.sh
#
# Then in SoundSync set "reachable address" to:  <PI_IP>:<PI_PORT>
set -eu

PI_USER="${PI_USER:-pi}"
PI_IP="${PI_IP:?set PI_IP to your Pi address}"
PI_PORT="${PI_PORT:-13337}"
LOCAL_PORT="${LOCAL_PORT:-3030}"

exec ssh -N \
  -o ServerAliveInterval=30 -o ServerAliveCountMax=3 -o ExitOnForwardFailure=yes \
  -R "0.0.0.0:${PI_PORT}:localhost:${LOCAL_PORT}" \
  "${PI_USER}@${PI_IP}"
