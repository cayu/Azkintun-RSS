#!/bin/bash
# =============================================================================
# undeploy.sh - Borra Azkintun-RSS del cluster k3s
#
# Por defecto borra el namespace entero (con TODO adentro: deployments,
# services, ingress, PVC con la base SQLite, y el Secret). Es la via mas
# limpia y completa.
#
# Uso:   bash k8s/3000-apps/azkintun/undeploy.sh
#        bash k8s/3000-apps/azkintun/undeploy.sh --keep-data   (conserva el PVC/DB)
#        bash k8s/3000-apps/azkintun/undeploy.sh --yes         (sin confirmacion)
#
# --keep-data: borra la app pero NO el PersistentVolumeClaim, para que al
#              volver a deployar recuperes tus feeds/leidos/favoritos.
# =============================================================================
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
NS="azkintun"

# Colores minimos (por si se corre sin el _lib).
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; RED='\033[0;31m'; NC='\033[0m'
log()  { echo -e "${GREEN}[OK]${NC} $1"; }
info() { echo -e "${BLUE}[->]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }
die()  { echo -e "${RED}[X]${NC} $1" >&2; exit 1; }

command -v kubectl &>/dev/null || die "kubectl no esta instalado."

KEEP_DATA="no"
ASSUME_YES="no"
for arg in "$@"; do
  case "$arg" in
    --keep-data) KEEP_DATA="yes" ;;
    --yes|-y)    ASSUME_YES="yes" ;;
    *) die "Opcion desconocida: $arg (validas: --keep-data, --yes)" ;;
  esac
done

if ! kubectl get namespace "$NS" &>/dev/null; then
  warn "El namespace '$NS' no existe. Nada que borrar."
  exit 0
fi

# Confirmacion (salvo --yes)
if [ "$ASSUME_YES" != "yes" ]; then
  if [ "$KEEP_DATA" = "yes" ]; then
    warn "Se borrara Azkintun del cluster PERO se conserva el PVC (la base SQLite)."
  else
    warn "Se borrara TODO el namespace '$NS', incluida la base SQLite (PVC)."
    warn "Esto elimina feeds marcados como leidos, favoritos y articulos scrapeados."
  fi
  read -rp "Continuar? [y/N] " ans
  case "$ans" in
    y|Y|yes|YES) ;;
    *) info "Cancelado."; exit 0 ;;
  esac
fi

if [ "$KEEP_DATA" = "yes" ]; then
  # Borrar todo MENOS el PVC. Se borran los recursos por tipo, preservando
  # el PersistentVolumeClaim azkintun-data.
  info "Borrando Ingress, Services, Deployments y Secret (conservando el PVC)..."
  kubectl delete ingress,middleware,deployment,service,secret --all -n "$NS" --ignore-not-found
  log "App borrada. El PVC 'azkintun-data' quedo intacto en el namespace '$NS'."
  info "Para recuperar todo: bash k8s/3000-apps/azkintun/deploy.sh --apply-only"
  info "Para borrar tambien los datos: kubectl delete namespace $NS"
else
  info "Borrando el namespace '$NS' completo..."
  kubectl delete namespace "$NS"
  log "Azkintun-RSS eliminado por completo del cluster."
fi
