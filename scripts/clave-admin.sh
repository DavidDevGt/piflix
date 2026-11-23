#!/bin/bash
# Uso:  sudo bash scripts/clave-admin.sh          -> muestra la clave actual
#       sudo bash scripts/clave-admin.sh nueva    -> genera una nueva y reinicia PiFlix
set -u
ENV_FILE="${PIFLIX_ENV:-/etc/piflix.env}"
[ "$(id -u)" = 0 ] || { echo "Ejecuta con sudo."; exit 1; }
[ -f "$ENV_FILE" ] || { echo "No existe $ENV_FILE"; exit 1; }

case "${1:-ver}" in
  ver)
    T=$(grep -m1 '^ADMIN_TOKEN=' "$ENV_FILE" | cut -d= -f2-)
    [ -n "$T" ] && echo "Clave actual: $T" || echo "No hay ADMIN_TOKEN en $ENV_FILE (usa: nueva)."
    ;;
  nueva)
    T=$(head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 24)
    [ "${#T}" -ge 12 ] || { echo "No se pudo generar la clave."; exit 1; }
    if grep -q '^ADMIN_TOKEN=' "$ENV_FILE"; then
      sed -i "s/^ADMIN_TOKEN=.*/ADMIN_TOKEN=$T/" "$ENV_FILE"
    else
      echo "ADMIN_TOKEN=$T" >> "$ENV_FILE"
    fi
    chmod 600 "$ENV_FILE"
    systemctl restart piflix
    sleep 2
    echo -n "Estado: "; curl -s localhost:8080/api/health; echo
    echo "Clave nueva: $T"
    ;;
  *)
    echo "Uso: sudo bash $0 [ver|nueva]"; exit 1 ;;
esac
