# =============================================================================
# Azkintun-RSS - Dockerfile
# Multi-stage build: compila el binario en un stage con toolchain de Rust,
# corre en un stage runtime mínimo (debian:slim).
# =============================================================================

# ─── Stage 1: Build ─────────────────────────────────────────────────────────
FROM rust:1-slim-bookworm AS builder

WORKDIR /app

# rusqlite (feature "bundled") compila SQLite desde su fuente C, y
# reqwest/native-tls necesita OpenSSL: hacen falta un compilador C y los
# headers de OpenSSL en tiempo de build.
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cachear dependencias: copiamos los manifiestos primero (incluido
# Cargo.lock, para un build reproducible con las versiones exactas
# probadas). `--locked` hace fallar el build si el lock está desfasado.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

# Ahora sí, el código real.
COPY src ./src
# Forzar recompilación del binario (el truco de arriba deja un binario
# placeholder cacheado con el mismo nombre).
RUN touch src/main.rs && cargo build --release --locked

# ─── Stage 2: Runtime ───────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# libssl3: librería de OpenSSL en runtime (native-tls la necesita dinámicamente).
# ca-certificates: para validar TLS al pegarle a los feeds RSS.
# curl: para el healthcheck de docker-compose.
# gosu: para que el entrypoint baje de root a uid 1000 tras ajustar permisos.
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 ca-certificates curl gosu \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/azkintun /usr/local/bin/azkintun

ENV PORT=3001
ENV AZKINTUN_DATA_DIR=/app/data
ENV SCRAPE_INTERVAL_MINUTES=15
# JWT_SECRET, ADMIN_USERNAME, ADMIN_PASSWORD, COOKIE_SECURE se pasan por
# docker-compose / entorno en runtime (no se hornean en la imagen).

# Usuario no-root (uid/gid 1000). Coincide con el securityContext del
# manifiesto k8s (runAsUser 1000 + fsGroup 1000).
RUN groupadd -g 1000 azkintun \
    && useradd -u 1000 -g 1000 -m -s /usr/sbin/nologin azkintun \
    && mkdir -p /app/data \
    && chown -R azkintun:azkintun /app/data

# Entrypoint que arregla los permisos del volumen montado antes de bajar
# privilegios. Cuando Docker monta un volumen (named o bind) sobre /app/data,
# puede quedar como root y tapar el chown del build; el entrypoint hace
# chown en runtime y luego ejecuta la app como uid 1000 con gosu.
# En Kubernetes esto NO se usa (el fsGroup del securityContext ya resuelve
# los permisos y el contenedor arranca directo como uid 1000).
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

EXPOSE 3001
VOLUME ["/app/data"]

HEALTHCHECK --interval=30s --timeout=10s --retries=5 --start-period=30s \
    CMD curl -f http://localhost:3001/api/health || exit 1

# NOTA: el entrypoint arranca como root para el chown y luego usa gosu para
# ejecutar como azkintun. No hay USER azkintun acá a propósito; el descenso
# de privilegios lo hace el entrypoint. En k8s, el securityContext fuerza
# uid 1000 igual, y el entrypoint detecta que ya no es root y saltea el chown.
ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["azkintun"]
