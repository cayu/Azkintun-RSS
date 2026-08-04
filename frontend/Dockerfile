# =============================================================================
# Azkintun-RSS - Dockerfile del contenedor frontend (nginx)
#
# Sirve el SPA estático y hace reverse-proxy de /api hacia el backend.
#
# Hoy sirve un placeholder (frontend/html/). Cuando exista el SPA real
# (React/Vite), agregar un stage de build de Node antes de este, algo así:
#
#   FROM node:20-slim AS build
#   WORKDIR /app
#   COPY package*.json ./
#   RUN npm ci
#   COPY . .
#   RUN npm run build          # genera /app/dist
#
# y luego copiar `--from=build /app/dist` en lugar de `html/`.
# =============================================================================

FROM nginx:1.27-alpine

# Config del server como TEMPLATE. La imagen oficial de nginx corre envsubst
# sobre los archivos de /etc/nginx/templates/*.template al arrancar y escribe
# el resultado en /etc/nginx/conf.d/. Así el upstream del backend se resuelve
# por entorno (Docker Compose vs Kubernetes) sin cambiar la imagen.
#
# NGINX_ENVSUBST_FILTER limita la sustitución a las variables propias
# (BACKEND_UPSTREAM y DNS_RESOLVER); sin esto, envsubst pisaría las variables
# internas de nginx ($host, $remote_addr, $proxy_add_x_forwarded_for, etc.).
ENV BACKEND_UPSTREAM=backend:3001
# Resolver DNS para la resolución perezosa del upstream (evita que nginx
# crashee si el backend aún no levantó). Default 127.0.0.11 = DNS embebido
# de Docker. En k8s se sobreescribe con el ClusterIP de CoreDNS (10.43.0.10).
ENV DNS_RESOLVER=127.0.0.11
ENV NGINX_ENVSUBST_FILTER='(BACKEND_UPSTREAM|DNS_RESOLVER)'

COPY nginx.conf.template /etc/nginx/templates/default.conf.template

# Archivos estáticos del SPA (placeholder por ahora).
COPY html/ /usr/share/nginx/html/

EXPOSE 80

HEALTHCHECK --interval=30s --timeout=5s --retries=3 --start-period=10s \
    CMD wget -q -O /dev/null http://localhost/ || exit 1
