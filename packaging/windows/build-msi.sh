#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
cargo zigbuild --offline --release --target x86_64-pc-windows-gnu
python3 packaging/windows/png2ico.py packaging/icon.png packaging/windows/icon.ico
mkdir -p dist
wixl -a x64 -D "Version=$version" -o dist/buildcraft3d-windows-x64.msi packaging/windows/buildcraft3d.wxs
