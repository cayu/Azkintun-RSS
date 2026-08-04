#!/bin/sh
# =============================================================================
# docker-entrypoint.sh - arranca Azkintun-RSS con permisos de datos correctos
#
# Problema que resuelve: cuando Docker monta un volumen (named o bind-mount)
# sobre /app/data, ese punto de montaje puede pertenecer a root y tapar el
# chown hecho en el build. Un proceso non-root (uid 1000) no podría entonces
# crear /app/data/azkintun.db → "unable to open database file".
#
# Solución: si el contenedor arranca como root (caso Docker Compose), este
# script hace chown del directorio de datos y luego ejecuta la app como el
# usuario 'azkintun' (uid 1000) usando gosu. Si el contenedor ya arranca como
# non-root (caso Kubernetes, donde el securityContext fuerza uid 1000 y el
# fsGroup ya resolvió los permisos del PVC), saltea el chown y ejecuta directo.
# =============================================================================
set -e

DATA_DIR="${AZKINTUN_DATA_DIR:-/app/data}"

if [ "$(id -u)" = "0" ]; then
    # Arrancamos como root: asegurar que el volumen sea escribible por uid 1000.
    mkdir -p "$DATA_DIR"
    chown -R azkintun:azkintun "$DATA_DIR" 2>/dev/null || true
    # Bajar privilegios y ejecutar la app como 'azkintun'. Si por algún
    # motivo gosu no estuviera disponible, se ejecuta como root (menos ideal
    # pero funcional) en vez de fallar el arranque.
    if command -v gosu >/dev/null 2>&1; then
        exec gosu azkintun "$@"
    else
        exec "$@"
    fi
else
    # Ya somos non-root (p. ej. Kubernetes con runAsUser 1000): ejecutar directo.
    exec "$@"
fi
