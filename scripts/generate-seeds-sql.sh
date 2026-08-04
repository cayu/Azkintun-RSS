#!/bin/bash
# =============================================================================
# generate-seeds-sql.sh - Regenera seeds.sql desde el estado real del binario
#
# seeds.sql es un dump de referencia (schema + carpetas + fuentes, sin
# articulos ni usuarios). Debe regenerarse cada vez que cambian los seeds en
# src/seeds_data.rs, para que no quede desactualizado.
#
# Este script:
#   1. Compila el binario (si hace falta).
#   2. Arranca una instancia efimera con una DB temporal (sin scrapear).
#   3. Vuelca schema + folders + sources a seeds.sql.
#
# Uso:   bash scripts/generate-seeds-sql.sh
#        bash scripts/generate-seeds-sql.sh --check   (falla si hay diferencias)
# =============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="${1:-}"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "[->] Compilando binario (si hace falta)..."
cargo build --quiet 2>/dev/null || cargo build

echo "[->] Sembrando una DB temporal..."
AZKINTUN_DATA_DIR="$TMPDIR" \
SCRAPE_ON_STARTUP=0 \
JWT_SECRET='generate-seeds-placeholder-32-caracteres-ok' \
ADMIN_PASSWORD=placeholder \
RUST_LOG=error \
PORT=39999 \
./target/debug/azkintun >"$TMPDIR/run.log" 2>&1 &
PID=$!
# Esperar a que la DB este sembrada
for i in $(seq 1 20); do
  [ -f "$TMPDIR/azkintun.db" ] && sleep 2 && break
  sleep 0.5
done
kill "$PID" 2>/dev/null || true
sleep 1

[ -f "$TMPDIR/azkintun.db" ] || { echo "[X] No se genero la DB"; cat "$TMPDIR/run.log"; exit 1; }

echo "[->] Volcando schema + folders + sources a seeds.sql..."
OUT="$TMPDIR/seeds.sql.new"
python3 - "$TMPDIR/azkintun.db" "$OUT" <<'PYEOF'
import sqlite3, sys
db, out = sys.argv[1], sys.argv[2]
conn = sqlite3.connect(db)
lines = [
    "-- Azkintun-RSS - dump de referencia de seeds",
    "-- Generado por scripts/generate-seeds-sql.sh - NO editar a mano.",
    "-- Contiene solo el schema y las carpetas/fuentes por defecto",
    "-- (sin articulos ni usuarios). Para levantar una DB limpia:",
    "--   sqlite3 nueva.db < seeds.sql",
    "",
    "PRAGMA journal_mode = WAL;",
    "PRAGMA foreign_keys = ON;",
    "",
]
# Schema (tablas, orden estable por nombre)
for (sql,) in conn.execute(
    "SELECT sql FROM sqlite_master WHERE type='table' "
    "AND name NOT IN ('sqlite_sequence') ORDER BY name"):
    if sql:
        lines.append(sql + ";")
        lines.append("")
# Indices
for (sql,) in conn.execute(
    "SELECT sql FROM sqlite_master WHERE type='index' "
    "AND sql IS NOT NULL ORDER BY name"):
    lines.append(sql + ";")
lines.append("")
# Folders
lines.append("-- Carpetas")
for fid, name in conn.execute("SELECT id, name FROM folders ORDER BY name"):
    name = name.replace("'", "''")
    lines.append(f"INSERT INTO folders (id, name) VALUES ({fid}, '{name}');")
lines.append("")
# Sources (solo las de fabrica: custom=0)
lines.append("-- Fuentes RSS por defecto")
for sid, name, url, fid, active, custom in conn.execute(
    "SELECT id, name, rss_url, folder_id, active, custom FROM sources "
    "WHERE custom = 0 ORDER BY folder_id, name"):
    name = (name or "").replace("'", "''")
    url = (url or "").replace("'", "''")
    fid = str(fid) if fid is not None else "NULL"
    lines.append(
        f"INSERT INTO sources (id, name, rss_url, folder_id, active, custom) "
        f"VALUES ({sid}, '{name}', '{url}', {fid}, {active}, {custom});")
lines.append("")
# Secuencias
lines.append("-- Restaurar secuencias de autoincrement")
for name, seq in conn.execute(
    "SELECT name, seq FROM sqlite_sequence WHERE name IN ('folders','sources')"):
    lines.append(f"INSERT OR REPLACE INTO sqlite_sequence (name, seq) VALUES ('{name}', {seq});")
open(out, "w").write("\n".join(lines) + "\n")
print(f"    {sum(1 for l in lines if l.startswith('INSERT INTO folders'))} folders, "
      f"{sum(1 for l in lines if l.startswith('INSERT INTO sources'))} sources")
PYEOF

if [ "$MODE" = "--check" ]; then
  if diff -q "$ROOT/seeds.sql" "$OUT" >/dev/null 2>&1; then
    echo "[OK] seeds.sql esta actualizado."
    exit 0
  else
    echo "[X] seeds.sql esta DESACTUALIZADO respecto a src/seeds_data.rs."
    echo "    Corrolo con: bash scripts/generate-seeds-sql.sh"
    diff "$ROOT/seeds.sql" "$OUT" | head -20 || true
    exit 1
  fi
else
  cp "$OUT" "$ROOT/seeds.sql"
  echo "[OK] seeds.sql regenerado."
fi
