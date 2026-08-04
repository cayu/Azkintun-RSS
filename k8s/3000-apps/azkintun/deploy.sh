#!/bin/bash
# =============================================================================
# deploy.sh - Despliega Azkintun-RSS (backend Rust + frontend nginx) en k3s
#
# Dos contenedores:
#   - backend  (Rust/Axum + SQLite en un PVC)  -> forgejo.local/apps/azkintun-backend
#   - frontend (nginx: SPA + reverse-proxy /api) -> forgejo.local/apps/azkintun-frontend
#
# El script:
#   1. Construye ambas imagenes para la arquitectura del cluster.
#   2. Las importa a containerd de k3s.
#   3. Crea el namespace y, si no existe, un Secret con JWT + admin generados.
#   4. Aplica los manifiestos (sin pisar el Secret real).
#
# Uso:   bash k8s/3000-apps/azkintun/deploy.sh
#        bash k8s/3000-apps/azkintun/deploy.sh --build-only   (solo imagenes)
#        bash k8s/3000-apps/azkintun/deploy.sh --apply-only   (solo manifiestos)
#
# Los helpers (build, secret, apply, wait) viven en _lib/deploy-common.sh.
# Se usa el del homelab si el deploy corre dentro de el; si no, la copia
# local incluida en este mismo directorio (para deployar solo con este repo).
# =============================================================================
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
# Raiz del repo Azkintun: tres niveles arriba (k8s/3000-apps/azkintun -> repo)
REPO_ROOT="$(cd "$DIR/../../.." && pwd)"
MODE="${1:-}"

# Cargar helpers: preferir el del homelab (../_lib) y si no, la copia local.
if [ -f "$DIR/../_lib/deploy-common.sh" ]; then
  source "$DIR/../_lib/deploy-common.sh"
elif [ -f "$DIR/_lib/deploy-common.sh" ]; then
  source "$DIR/_lib/deploy-common.sh"
else
  echo "[X] No se encontro deploy-common.sh (ni en ../_lib ni en ./_lib)" >&2
  exit 1
fi
enable_error_trap

# Config de publicación (con defaults). Se usan en apply_manifests y en el
# mensaje final. Podés sobreescribirlas por entorno:
#   AZKINTUN_HOST=azkintun.midominio.com AZKINTUN_ISSUER=letsencrypt-prod ./deploy.sh
#   AZKINTUN_TLS=false ./deploy.sh   (solo HTTP:80)
HOST="${AZKINTUN_HOST:-azkintun.local}"
ISSUER="${AZKINTUN_ISSUER:-selfsigned-issuer}"
TLS="${AZKINTUN_TLS:-true}"

echo "=============================================================="
echo "   Azkintun-RSS - deploy en k3s (backend + frontend)"
echo "=============================================================="

build_images() {
  info "Construyendo imagenes desde $REPO_ROOT"
  # Backend desde la raiz del repo (Dockerfile en la raiz); frontend desde frontend/.
  build_and_import azkintun-backend  "$REPO_ROOT"
  build_and_import azkintun-frontend "$REPO_ROOT/frontend"
}

apply_manifests() {
  kubectl create namespace azkintun --dry-run=client -o yaml | kubectl apply -f -

  # Secret: si no existe, generar JWT_SECRET y ADMIN_PASSWORD aleatorios.
  if ! kubectl get secret azkintun-secret -n azkintun &>/dev/null; then
    info "Generando JWT_SECRET y ADMIN_PASSWORD aleatorios..."
    local jwt pass
    jwt=$(head -c 48 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 48)
    pass=$(head -c 18 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 20)
    ensure_secret azkintun azkintun-secret \
      "JWT_SECRET=$jwt" \
      "ADMIN_USERNAME=admin" \
      "ADMIN_PASSWORD=$pass"
    echo ""
    warn "Credenciales del admin (guardalas, no se vuelven a mostrar):"
    echo "    usuario:  admin"
    echo "    password: $pass"
    echo ""
  fi

  # ── Parametrizar host / issuer / TLS ────────────────────────────────────────
  # Usa las variables globales HOST / ISSUER / TLS (definidas arriba).
  local manifest
  manifest="$(mktemp)"

  # Partimos del manifiesto y sustituimos los defaults por los valores dados.
  sed -e "s/azkintun\.local/${HOST}/g" \
      -e "s/selfsigned-issuer/${ISSUER}/g" \
      "$DIR/k8s/manifests.yaml" > "$manifest"

  info "Publicando en host: ${HOST}  (issuer: ${ISSUER}, TLS: ${TLS})"

  if [ "$TLS" != "true" ]; then
    # Modo solo-HTTP: quitar el bloque tls, la annotation de redirect, el
    # cluster-issuer y el Middleware, para que Traefik sirva en :80 sin TLS.
    warn "TLS deshabilitado: la app se publica solo en HTTP (:80), sin redirect."
    python3 - "$manifest" <<'PYEOF'
import sys, yaml
path = sys.argv[1]
docs = [d for d in yaml.safe_load_all(open(path)) if d]
out = []
for d in docs:
    if d.get("kind") == "Middleware" and d["metadata"]["name"] == "redirect-https":
        continue  # no hace falta el redirect
    if d.get("kind") == "Ingress":
        ann = d["metadata"].get("annotations", {})
        ann.pop("traefik.ingress.kubernetes.io/router.middlewares", None)
        ann.pop("cert-manager.io/cluster-issuer", None)
        d["metadata"]["annotations"] = ann
        d["spec"].pop("tls", None)
    out.append(d)
yaml.safe_dump_all(out, open(path, "w"))
PYEOF
  fi

  # Aplicar filtrando el Secret (el real ya existe en el cluster).
  apply_without_secret "$manifest"
  rm -f "$manifest"

  wait_rollout deployment azkintun-backend  azkintun
  wait_rollout deployment azkintun-frontend azkintun
}

preflight

case "$MODE" in
  --build-only) build_images ;;
  --apply-only) apply_manifests ;;
  *)            build_images; apply_manifests ;;
esac

echo ""
if [ "$TLS" = "true" ]; then
  info "Acceso web (apuntá el host a la EXTERNAL-IP de Traefik):"
  echo "    https://${HOST}"
else
  info "Acceso web (apuntá el host a la EXTERNAL-IP de Traefik):"
  echo "    http://${HOST}     (publicado sin TLS)"
fi
info "IP de Traefik para tu /etc/hosts o DNS:"
echo "    kubectl -n kube-system get svc traefik -o jsonpath='{.status.loadBalancer.ingress[0].ip}'"
info "O sin tocar el Ingress ni el DNS, por port-forward:"
echo "    kubectl port-forward -n azkintun svc/azkintun-frontend 8080:80 &"
echo "    abrir http://localhost:8080"
info "Forzar un scrape manual de RSS (o desde la UI, boton Actualizar):"
echo "    kubectl exec -n azkintun deploy/azkintun-backend -- curl -s -X POST http://localhost:3001/api/scrape"
info "Logs:  kubectl logs -n azkintun deploy/azkintun-backend -f"
