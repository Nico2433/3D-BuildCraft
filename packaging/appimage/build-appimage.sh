#!/usr/bin/env bash
# Builds dist/buildcraft3d-linux-x86_64.AppImage. The binary is linked against glibc 2.31
# with cargo-zigbuild so it runs on older distributions too.
# Needs zig, cargo-zigbuild and appimagetool (APPIMAGETOOL, default on PATH); RUNTIME may
# name a type2 runtime file so appimagetool works offline.
set -euo pipefail
cd "$(dirname "$0")/../.."
id=io.github.nico2433.BuildCraft3D
target=x86_64-unknown-linux-gnu
cargo zigbuild --offline --release --target "$target.2.31"

app=target/appimage/AppDir
rm -rf "$app"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"
install -m755 "target/$target/release/buildcraft3d" "$app/usr/bin/buildcraft3d"
install -m644 "packaging/flatpak/$id.desktop" "$app/usr/share/applications/$id.desktop"
install -m644 packaging/icon.png "$app/usr/share/icons/hicolor/256x256/apps/$id.png"
ln -s "usr/share/applications/$id.desktop" "$app/$id.desktop"
ln -s "usr/share/icons/hicolor/256x256/apps/$id.png" "$app/$id.png"
ln -s "$id.png" "$app/.DirIcon"
cat >"$app/AppRun" <<'EOF'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/buildcraft3d" "$@"
EOF
chmod +x "$app/AppRun"

mkdir -p dist
ARCH=x86_64 "${APPIMAGETOOL:-appimagetool}" ${RUNTIME:+--runtime-file "$RUNTIME"} \
    "$app" dist/buildcraft3d-linux-x86_64.AppImage
