#!/usr/bin/env bash
# Builds dist/buildcraft3d-<version>-<rel>-x86_64.pkg.tar.zst for pacman -U from the
# committed state of this checkout (git archive HEAD), not from the release tag.
# Needs makepkg (base-devel), cargo and clang. The compiler flags are reset to generic
# x86-64 so a host makepkg.conf with -march=native / target-cpu=native does not leak in.
set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD
ver=$(git show HEAD:Cargo.toml | sed -n 's/^version = "\(.*\)"/\1/p' | head -n1)
[ -n "$ver" ] || { echo "no version in Cargo.toml" >&2; exit 1; }

work=target/arch
rm -rf "$work"
mkdir -p "$work"
# Same name and top folder as GitHub's tag tarball, so makepkg uses it instead of downloading.
git archive --format=tar.gz --prefix="3D-BuildCraft-$ver/" -o "$work/buildcraft3d-$ver.tar.gz" HEAD
sed "s/^pkgver=.*/pkgver=$ver/" packaging/arch/PKGBUILD >"$work/PKGBUILD"

# The system makepkg.conf (and its .d files) with portable flags on top.
conf="$root/$work/makepkg.conf"
cat /etc/makepkg.conf /etc/makepkg.conf.d/*.conf >"$conf" 2>/dev/null || cat /etc/makepkg.conf >"$conf"
cat >>"$conf" <<'EOF'
CARCH="x86_64"
CHOST="x86_64-pc-linux-gnu"
CFLAGS="-march=x86-64 -mtune=generic -O2 -pipe -fno-plt -fexceptions \
        -Wp,-D_FORTIFY_SOURCE=3 -Wformat -Werror=format-security \
        -fstack-clash-protection -fcf-protection"
CXXFLAGS="$CFLAGS -Wp,-D_GLIBCXX_ASSERTIONS"
RUSTFLAGS=""
PKGDEST=""
SRCDEST=""
BUILDDIR=""
PKGEXT=".pkg.tar.zst"
EOF

cd "$work"
updpkgsums
args=(-f --config "$conf")
# A cargo from a user-level rustup is fine but no package provides it: skip makepkg's
# dependency check then, as long as every other dependency is installed.
if ! pacman -T cargo >/dev/null && command -v cargo >/dev/null; then
    missing=$(bash -c 'source PKGBUILD; pacman -T "${depends[@]}" "${makedepends[@]}"' | grep -vx cargo || true)
    [ -z "$missing" ] || { echo "missing packages: $missing" >&2; exit 1; }
    args+=(--nodeps)
fi
# shellcheck disable=SC2086
makepkg "${args[@]}" ${MAKEPKG_ARGS:-}

pkg=$(ls -t buildcraft3d-"$ver"-*-x86_64.pkg.tar.zst | grep -v -- -debug- | head -n1)
mkdir -p "$root/dist"
cp "$pkg" "$root/dist/"
echo "dist/$pkg"
