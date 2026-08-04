#!/bin/bash
# =============================================================================
# _lib/deploy-common.sh - Helpers compartidos para los deploy.sh de las apps
#
# Centraliza el manejo de errores, los chequeos de precondiciones y la
# detección de plataforma para que cada app no repita lo mismo.
#
# Uso en un deploy.sh:
#   source "$(dirname "$0")/../_lib/deploy-common.sh"
#   preflight                 # valida kubectl, cluster, builder
#   PLATFORM="$(detect_platform)"
#   build_and_import nombre /ruta/al/contexto
# =============================================================================

# Colores y logging
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; RED='\033[0;31m'; NC='\033[0m'
log()  { echo -e "${GREEN}[✓]${NC} $1"; }
info() { echo -e "${BLUE}[→]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }
err()  { echo -e "${RED}[✗]${NC} $1" >&2; }
die()  { err "$1"; exit 1; }

# Trap: si algo falla, decir en qué línea (mucho más útil que un error mudo)
_on_error() {
  local exit_code=$?
  err "Falló en la línea $1 (código $exit_code)."
  err "Revisá el mensaje de arriba. Para diagnóstico del cluster: make check"
  exit "$exit_code"
}
enable_error_trap() {
  set -euo pipefail
  trap '_on_error $LINENO' ERR
}

# ── Detección de plataforma (arquitectura del contenedor) ─────────────────────
detect_platform() {
  case "$(uname -m)" in
    x86_64|amd64)  echo "linux/amd64" ;;
    aarch64|arm64) echo "linux/arm64" ;;
    *)             echo "linux/$(uname -m)" ;;
  esac
}

# ── Chequeo de precondiciones antes de desplegar ──────────────────────────────
# Evita los errores crípticos típicos: "command not found", "connection refused".
preflight() {
  local need_builder="${1:-yes}"

  # kubectl presente
  command -v kubectl &>/dev/null || die "kubectl no está instalado o no está en PATH."

  # cluster accesible
  if ! kubectl cluster-info &>/dev/null; then
    die "No se puede contactar el cluster. ¿k3s está corriendo? Probá: make status"
  fi

  # builder de imágenes (docker o nerdctl), solo si la app lo necesita
  if [ "$need_builder" = "yes" ]; then
    if ! command -v docker &>/dev/null && ! command -v nerdctl &>/dev/null; then
      die "Ni docker ni nerdctl encontrados. Instalá uno para construir imágenes."
    fi
  fi

  log "Precondiciones OK (kubectl, cluster accesible)"
}

# ── Build + import de una imagen a containerd de k3s ──────────────────────────
# build_and_import <nombre-imagen> <contexto> [tag]
build_and_import() {
  local name="$1" ctx="$2" tag="${3:-latest}"
  local image="forgejo.local/apps/${name}:${tag}"
  local platform; platform="$(detect_platform)"

  [ -d "$ctx" ] || die "Contexto de build no existe: $ctx"
  [ -f "$ctx/Dockerfile" ] || die "No hay Dockerfile en: $ctx"

  info "Build $name ($platform)..."
  if command -v docker &>/dev/null; then
    docker build --platform "$platform" -t "$image" "$ctx" \
      || die "Falló el build de $name. Revisá el Dockerfile y los logs de arriba."
    info "Importando $name a containerd..."
    docker save "$image" | sudo k3s ctr images import - \
      || die "Falló la importación de $name a k3s. ¿Tenés permisos sudo para k3s?"
  elif command -v nerdctl &>/dev/null; then
    nerdctl build --platform "$platform" -t "$image" "$ctx" \
      || die "Falló el build de $name con nerdctl."
  fi
  log "$name listo ($image)"
}

# ── Crear un Secret de forma segura si no existe ──────────────────────────────
# ensure_secret <namespace> <nombre-secret> KEY1=valor KEY2=valor ...
ensure_secret() {
  local ns="$1" name="$2"; shift 2
  if kubectl get secret "$name" -n "$ns" &>/dev/null; then
    log "Secret $name ya existe (no se sobrescribe)"
    return 0
  fi
  local args=()
  for kv in "$@"; do args+=(--from-literal="$kv"); done
  kubectl create secret generic "$name" -n "$ns" "${args[@]}" \
    || die "No se pudo crear el Secret $name."
  log "Secret $name creado (no queda en Git)"
}

# ── Aplicar manifiestos FILTRANDO el bloque Secret (para no pisar el real) ─────
apply_without_secret() {
  local manifest="$1"
  [ -f "$manifest" ] || die "Manifiesto no encontrado: $manifest"
  local tmp; tmp="$(mktemp)"
  python3 -c "
import sys, yaml
docs = [d for d in yaml.safe_load_all(open('$manifest')) if d and d.get('kind') != 'Secret']
yaml.safe_dump_all(docs, open('$tmp','w'))
" || die "Falló el filtrado del manifiesto (¿python3 + pyyaml disponibles?)"
  kubectl apply -f "$tmp" || die "Falló kubectl apply de $manifest"
  rm -f "$tmp"
}

# ── Esperar un rollout con mensaje claro si falla ─────────────────────────────
wait_rollout() {
  local kind="$1" name="$2" ns="$3" timeout="${4:-120s}"
  info "Esperando $kind/$name..."
  if ! kubectl rollout status "$kind/$name" -n "$ns" --timeout="$timeout"; then
    err "$kind/$name no quedó listo en $timeout."
    err "Diagnóstico: kubectl get pods -n $ns ; kubectl describe $kind/$name -n $ns"
    err "Logs:        kubectl logs -n $ns -l app=$name --tail=50"
    die "Deploy incompleto."
  fi
  log "$kind/$name desplegado ✓"
}
