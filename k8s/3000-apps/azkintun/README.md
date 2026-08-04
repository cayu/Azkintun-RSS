# Azkintun-RSS — deploy en k3s

Despliegue de **Azkintun-RSS** en el cluster: agregador de noticias de
ciberseguridad propio (Rust/Axum + SQLite, con frontend nginx), con auth,
import/export OPML/CSV y ~1400 feeds de fábrica.

Es la evolución de la vieja app de ejemplo `cyberpulse` (que era React +
Express de un tercero): reescrita en Rust, ahora es una app **propia**. Sirve
además como ejemplo de deploy de una app **de dos contenedores y stateful**
(backend + frontend + PVC).

## Arquitectura

```
Browser ─▶ Ingress (traefik, TLS) ─▶ Service azkintun-frontend (nginx :80)
                                          │
                                          ├─ sirve el SPA estático
                                          └─ proxy /api ─▶ Service azkintun-backend (:3001)
                                                                │
                                                                └─▶ PVC (SQLite azkintun.db)
                                                                     scrape cada 15 min ─▶ ~1400 feeds
```

El browser solo habla con el frontend (mismo origen) → sin CORS, y el backend
nunca queda expuesto directamente al Ingress.

## Contenido de esta carpeta

```
azkintun/
├── README.md                         este archivo
├── deploy.sh                         build + import + deploy
├── undeploy.sh                       borrado (namespace, o solo la app)
├── _lib/deploy-common.sh             helpers (copia; usa la del homelab si está)
├── k8s/manifests.yaml                todos los recursos k8s
└── .forgejo/workflows/
    └── build-and-deploy.yml          CI/CD de Forgejo Actions
```

---

## Requisitos previos

- Un cluster **k3s** en marcha (`kubectl cluster-info` responde).
- **docker** o **nerdctl** para construir las imágenes (las imágenes NO están
  en ningún registry público: se construyen desde este repo).
- **Ingress traefik** (viene con k3s) y, si querés TLS, **cert-manager** con un
  `ClusterIssuer` llamado `selfsigned-issuer` (como el resto del homelab). Si
  no tenés cert-manager, podés acceder por `port-forward` sin tocar el Ingress.
- `python3` con `pyyaml` (lo usa el filtrado del Secret y el modo sin-TLS del
  deploy). Si falta: `pip install pyyaml` o `sudo apt install python3-yaml`.
- **Arquitectura arm64** (Raspberry Pi). Los manifiestos fijan
  `nodeSelector: kubernetes.io/arch: arm64` y el `deploy.sh` construye las
  imágenes para la arquitectura del nodo donde corre. Para un cluster x86,
  cambiá el `nodeSelector` a `amd64` (o quitalo) en `k8s/manifests.yaml` y
  construí con `--platform linux/amd64`. Ver la wiki 18 (portabilidad) del
  homelab para la guía de migración completa.

---

## Deploy

### Opción A — todo automático (recomendada)

```bash
bash k8s/3000-apps/azkintun/deploy.sh
```

Hace, en orden:

1. **Chequea precondiciones** (kubectl, cluster accesible, builder disponible).
2. **Construye** las dos imágenes para la arquitectura del cluster
   (`azkintun-backend` desde la raíz del repo, `azkintun-frontend` desde
   `frontend/`).
3. **Importa** ambas a containerd de k3s (`k3s ctr images import`).
4. **Crea** el namespace `azkintun`.
5. Si no existe, **genera un Secret** con `JWT_SECRET` y `ADMIN_PASSWORD`
   aleatorios y **te muestra las credenciales una sola vez** (guardalas).
6. **Aplica** los manifiestos (sin pisar el Secret real) y espera a que ambos
   deployments estén listos.

> En una Pi, el build del backend Rust tarda varios minutos la primera vez
> (compila el binario y SQLite desde fuente). Los builds siguientes usan caché.

### Publicar en 80 / 443 (host, TLS e issuer)

Traefik (que viene con k3s) ya escucha en **80 y 443**, con la EXTERNAL-IP que
le asigna MetalLB. El `deploy.sh` acepta tres variables de entorno para elegir
cómo se publica la app, **sin editar el YAML**:

| Variable | Default | Qué hace |
|---|---|---|
| `AZKINTUN_HOST` | `azkintun.local` | El host del Ingress (tu dominio o un `nip.io`). |
| `AZKINTUN_ISSUER` | `selfsigned-issuer` | ClusterIssuer de cert-manager. Usá `letsencrypt-prod` para un cert válido en un dominio público. |
| `AZKINTUN_TLS` | `true` | `true` = 443 con TLS + redirect 80→443. `false` = solo 80 (HTTP), sin TLS ni redirect. |

**Caso 1 — LAN con HTTPS self-signed (default).** Publica en 443 con un cert
auto-firmado y redirige 80→443. El browser avisará que el cert no es de una CA
conocida (normal en LAN):

```bash
AZKINTUN_HOST=azkintun.local bash k8s/3000-apps/azkintun/deploy.sh
```

**Caso 2 — dominio público con Let's Encrypt (cert válido en 443).** Requiere
que el dominio resuelva a la IP pública de tu cluster y que el puerto 80 esté
accesible desde internet (para el challenge HTTP-01):

```bash
AZKINTUN_HOST=azkintun.midominio.com \
AZKINTUN_ISSUER=letsencrypt-prod \
bash k8s/3000-apps/azkintun/deploy.sh
```

**Caso 3 — solo HTTP en el puerto 80 (sin TLS).** Para pruebas rápidas o si el
TLS lo termina otro proxy más adelante. Quita el bloque `tls`, el redirect y el
Middleware:

```bash
AZKINTUN_HOST=azkintun.local AZKINTUN_TLS=false \
bash k8s/3000-apps/azkintun/deploy.sh
```

Para que el host resuelva en tu LAN, agregá una línea a `/etc/hosts` (o a tu
DNS local) apuntando el host a la EXTERNAL-IP de Traefik:

```bash
# IP que MetalLB le dio a Traefik:
kubectl -n kube-system get svc traefik -o jsonpath='{.status.loadBalancer.ingress[0].ip}'
# luego en /etc/hosts:   192.168.2.200  azkintun.local
```

> **Nota sobre la cookie de auth.** El backend usa `COOKIE_SECURE=false` porque
> el TLS lo termina Traefik: el browser habla HTTPS con Traefik, pero Traefik
> habla HTTP plano con la app dentro del cluster. La cookie funciona igual en
> los tres casos. No la pongas en `true` salvo que expongas el backend
> directamente por HTTPS (no es el caso con este Ingress).

### Opción B — por partes

```bash
# Solo construir e importar las imágenes
bash k8s/3000-apps/azkintun/deploy.sh --build-only

# Solo aplicar los manifiestos (si las imágenes ya están importadas)
bash k8s/3000-apps/azkintun/deploy.sh --apply-only
```

### Opción C — a mano (para entender cada paso)

```bash
# 1. Construir (ajustá --platform a tu arquitectura: arm64 en la Pi)
docker build --platform linux/arm64 -t forgejo.local/apps/azkintun-backend:latest .
docker build --platform linux/arm64 -t forgejo.local/apps/azkintun-frontend:latest frontend/

# 2. Importar a k3s
docker save forgejo.local/apps/azkintun-backend:latest  | sudo k3s ctr images import -
docker save forgejo.local/apps/azkintun-frontend:latest | sudo k3s ctr images import -

# 3. Namespace + Secret (JWT mínimo 32 chars; admin a tu gusto).
#    IMPORTANTE: creá el Secret ANTES de aplicar, para no dejar el placeholder.
kubectl create namespace azkintun
kubectl create secret generic azkintun-secret -n azkintun \
  --from-literal=JWT_SECRET="$(openssl rand -hex 32)" \
  --from-literal=ADMIN_USERNAME=admin \
  --from-literal=ADMIN_PASSWORD="$(openssl rand -base64 16)"

# 4. Aplicar. El bloque Secret del YAML es solo un placeholder de demo;
#    como el Secret real ya existe, filtralo para no sobreescribirlo:
python3 -c "import yaml,sys; docs=[d for d in yaml.safe_load_all(open('k8s/3000-apps/azkintun/k8s/manifests.yaml')) if d and d.get('kind')!='Secret']; yaml.safe_dump_all(docs, sys.stdout)" | kubectl apply -f -
```

> Si aplicás el `manifests.yaml` tal cual con `kubectl apply -f` (sin filtrar),
> se creará el Secret con el placeholder `REEMPLAZAR_...` y el login fallará.
> Usá el `deploy.sh` o filtrá el Secret como arriba.
>
> El manifiesto aplica con los defaults: host `azkintun.local`, issuer
> `selfsigned-issuer`, TLS en 443 con redirect desde 80 (incluye el `Middleware`
> `redirect-https`). Para otro host/issuer, cambialos con `sed` antes de aplicar
> (`s/azkintun.local/tu.host/g`, `s/selfsigned-issuer/letsencrypt-prod/g`), o
> mejor usá el `deploy.sh` con las variables `AZKINTUN_HOST` / `AZKINTUN_ISSUER`
> / `AZKINTUN_TLS` (ver "Publicar en 80 / 443").

### Verificar que quedó bien

```bash
kubectl get pods,svc,ingress,middleware -n azkintun
kubectl rollout status deployment/azkintun-backend  -n azkintun
kubectl rollout status deployment/azkintun-frontend -n azkintun
```

---

## Acceso

```bash
# Web, con el host que definiste (AZKINTUN_HOST) apuntando a la IP de Traefik:
https://azkintun.local          # con TLS (casos 1 y 2)
http://azkintun.local           # si publicaste con AZKINTUN_TLS=false (caso 3)

# O sin tocar el Ingress ni el DNS, por port-forward directo al frontend:
kubectl port-forward -n azkintun svc/azkintun-frontend 8080:80
#   → http://localhost:8080
```

El usuario es `admin` y la contraseña es la que mostró el `deploy.sh` (o la que
pusiste vos si creaste el Secret a mano).

---

## Operación

```bash
# Forzar un scrape manual de RSS (o desde la UI, botón "Actualizar")
kubectl exec -n azkintun deploy/azkintun-backend -- \
  curl -s -X POST http://localhost:3001/api/scrape

# Logs
kubectl logs -n azkintun deploy/azkintun-backend  -f
kubectl logs -n azkintun deploy/azkintun-frontend -f

# Reiniciar (por ejemplo tras cambiar el Secret)
kubectl rollout restart deployment/azkintun-backend -n azkintun
```

### Backup / restore de la base

La base entera es un archivo SQLite en el PVC:

```bash
# Backup
kubectl exec -n azkintun deploy/azkintun-backend -- \
  cat /app/data/azkintun.db > azkintun-backup-$(date +%F).db

# Restore: escalar a 0, copiar el archivo, volver a escalar a 1
kubectl scale deployment/azkintun-backend -n azkintun --replicas=0
POD=$(kubectl get pod -n azkintun -l app=azkintun-backend -o name)  # (con réplicas>0)
# ...con el pod activo, kubectl cp azkintun-backup.db azkintun/POD:/app/data/azkintun.db
kubectl scale deployment/azkintun-backend -n azkintun --replicas=1
```

---

## Borrado

### Opción A — script (recomendada)

```bash
# Borra TODO el namespace: app + PVC (base SQLite) + Secret. Pide confirmación.
bash k8s/3000-apps/azkintun/undeploy.sh

# Borra la app pero CONSERVA el PVC (para recuperar feeds/leídos al re-deployar)
bash k8s/3000-apps/azkintun/undeploy.sh --keep-data

# Sin pregunta de confirmación (para scripts/CI)
bash k8s/3000-apps/azkintun/undeploy.sh --yes
```

### Opción B — a mano

```bash
# Borrado total (incluye la base SQLite del PVC):
kubectl delete namespace azkintun

# O borrar solo la app conservando los datos (el PVC):
kubectl delete ingress,deployment,service,secret --all -n azkintun
#   → para recuperar: bash k8s/3000-apps/azkintun/deploy.sh --apply-only
```

> **Ojo:** borrar el namespace (o el PVC) elimina la base SQLite: se pierden
> los artículos scrapeados y el estado de leídos/favoritos. Hacé un backup
> antes si te importa (ver sección Backup). Los feeds por defecto se vuelven a
> sembrar solos en el próximo deploy, así que eso no se pierde.

---

## Configuración

Toda la config va por variables de entorno del backend (ver el `Deployment`
en `k8s/manifests.yaml`):

| Variable | Default (k8s) | Para qué |
|---|---|---|
| `JWT_SECRET` | *(del Secret)* | Firma de los JWT. Mínimo 32 caracteres. **Obligatoria.** |
| `ADMIN_USERNAME` / `ADMIN_PASSWORD` | *(del Secret)* | Usuario admin creado en el primer arranque. |
| `AZKINTUN_DATA_DIR` | `/app/data` | Dónde vive la SQLite (montado desde el PVC). |
| `COOKIE_SECURE` | `false` | `false` porque el TLS lo termina el Ingress. |
| `SCRAPE_INTERVAL_MINUTES` | `15` | Cada cuánto scrapea los feeds. |
| `SCRAPE_CONCURRENCY` | `16` | Feeds en paralelo (importante con ~1400). |
| `SCRAPE_ON_STARTUP` | `0` | `1` para poblar feeds al arrancar. |

El frontend tiene dos variables propias (nginx):

| Variable | Default (k8s) | Para qué |
|---|---|---|
| `BACKEND_UPSTREAM` | `azkintun-backend:3001` | A qué Service proxea el `/api`. |
| `DNS_RESOLVER` | `10.43.0.10` | ClusterIP de CoreDNS, para resolución perezosa del upstream. |

> **Nota sobre el upstream nginx.** La imagen del frontend es la misma para
> Docker Compose y k8s: el destino del backend se parametriza con
> `BACKEND_UPSTREAM` (default `backend:3001` para Compose). Además, nginx hace
> **resolución perezosa** del DNS del backend (via `DNS_RESOLVER`), así el pod
> del frontend **no crashea si el backend todavía no levantó** - simplemente
> devuelve 502 hasta que el backend está listo, y ahí empieza a proxear. Si
> ves 502 permanentes en `/api`: verificá que `BACKEND_UPSTREAM` sea el nombre
> real del Service, y que `DNS_RESOLVER` sea el ClusterIP de tu CoreDNS
> (`kubectl -n kube-system get svc kube-dns -o jsonpath='{.spec.clusterIP}'`).

---

## CI/CD (Forgejo Actions)

`.forgejo/workflows/build-and-deploy.yml` construye ambas imágenes, corre un
test de humo del backend (`/api/health`), las importa a k3s y aplica los
manifiestos **sin** tocar el Secret (que ya vive en el cluster). Se dispara al
pushear cambios de `src/`, `frontend/`, `Dockerfile` o los manifiestos.

Para que el CI respete tu host/issuer/TLS (y no revierta al default
`azkintun.local` en cada push), definí estas **Variables** del repo en Forgejo
(*Settings → Actions → Variables*):

| Variable | Ejemplo |
|---|---|
| `AZKINTUN_HOST` | `azkintun.midominio.com` |
| `AZKINTUN_ISSUER` | `letsencrypt-prod` |
| `AZKINTUN_TLS` | `true` |

Si no las definís, el CI usa los mismos defaults que el `deploy.sh`.

---

## Migración desde `cyberpulse`

Si tenías la vieja app `cyberpulse` corriendo, son apps distintas (namespaces
distintos): no hay migración automática de datos porque el esquema cambió por
completo (Rust vs Node). Podés exportar tus feeds de la vieja e importarlos en
Azkintun por su UI o `POST /api/import/opml`. Cuando confirmes que Azkintun
anda, `kubectl delete namespace cyberpulse`.
