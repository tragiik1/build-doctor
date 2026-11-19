#!/usr/bin/env bash
# Spike: build the exact Lua runtime PoB2 needs and fetch PoB2, so the
# headless benchmarks in this folder can run. Linux/macOS only for now.
#
# Why a pinned LuaJIT: PoB2's dev branch uses compound assignment (`x += 1`),
# which upstream LuaJIT only gained in 2026. Distro LuaJIT (e.g. Ubuntu's
# 2023 build) fails to parse Modules/Main.lua. PoB2 pins this commit in
# PathOfBuilding-SimpleGraphic/vcpkg-ports/ports/luajit/2026-07-20_1.
set -euo pipefail

DEST="${1:-./engine}"
LUAJIT_COMMIT=2460b3ff93a1c955de3d62cfc825de7d68dc272e
POB2_REF="${POB2_REF:-dev}"

mkdir -p "$DEST" && cd "$DEST"

[ -d luajit ] || git clone -q https://github.com/LuaJIT/LuaJIT luajit
git -C luajit checkout -q "$LUAJIT_COMMIT"
make -C luajit -j"$(nproc 2>/dev/null || echo 4)" >/dev/null

[ -d luautf8 ] || git clone -q --depth 1 https://github.com/starwing/luautf8
cc -O2 -shared -fPIC -Iluajit/src luautf8/lutf8lib.c -o luautf8/lua-utf8.so

[ -d pob2 ] || git clone -q --depth 50 --branch "$POB2_REF" \
  https://github.com/PathOfBuildingCommunity/PathOfBuilding-PoE2 pob2

cat <<EOF
Ready. Run a spike from pob2/src:
  cd $DEST/pob2/src
  LUA_PATH="../runtime/lua/?.lua;../runtime/lua/?/init.lua;;" \\
  LUA_CPATH="$PWD/luautf8/?.so;;" \\
  $PWD/luajit/src/luajit /path/to/research/spikes/roundtrip.lua
EOF
