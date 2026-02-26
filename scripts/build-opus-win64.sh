#!/usr/bin/env bash
#
# Pre-build libopus for Windows x86_64 using mingw-w64.
#
# audiopus_sys's build.rs doesn't pass --host to autotools configure,
# so cross-compilation produces Linux objects.  This script builds opus
# separately with the correct cross-compiler, and the OPUS_LIB_DIR env
# var makes audiopus_sys skip its own build entirely.
#
# Usage:
#   ./scripts/build-opus-win64.sh
#   OPUS_LIB_DIR=$PWD/opus-win64/lib LIBOPUS_STATIC=1 \
#     cargo build --manifest-path src-tauri/Cargo.toml \
#       --target x86_64-pc-windows-gnu --release

set -euo pipefail

OPUS_VERSION="1.5.2"
PREFIX="$(pwd)/opus-win64"
BUILD_DIR="$(mktemp -d)"
CROSS_HOST="x86_64-w64-mingw32"

echo "==> Building libopus ${OPUS_VERSION} for ${CROSS_HOST}"
echo "    prefix : ${PREFIX}"
echo "    tmpdir : ${BUILD_DIR}"

# Check prerequisites
for tool in ${CROSS_HOST}-gcc make autoconf automake libtool; do
    if ! command -v "$tool" &>/dev/null; then
        echo "ERROR: $tool not found. Install mingw-w64 and autotools."
        exit 1
    fi
done

# Download & extract
cd "$BUILD_DIR"
if command -v curl &>/dev/null; then
    curl -sSL "https://downloads.xiph.org/releases/opus/opus-${OPUS_VERSION}.tar.gz" -o opus.tar.gz
elif command -v wget &>/dev/null; then
    wget -q "https://downloads.xiph.org/releases/opus/opus-${OPUS_VERSION}.tar.gz" -O opus.tar.gz
else
    echo "ERROR: neither curl nor wget found"
    exit 1
fi
tar xzf opus.tar.gz
cd "opus-${OPUS_VERSION}"

# Configure for cross-compilation
./configure \
    --host="${CROSS_HOST}" \
    --prefix="${PREFIX}" \
    --enable-static \
    --disable-shared \
    --disable-doc \
    --disable-extra-programs \
    --with-pic \
    CC="${CROSS_HOST}-gcc" \
    AR="${CROSS_HOST}-ar" \
    RANLIB="${CROSS_HOST}-ranlib"

# Build & install
make -j"$(nproc)"
make install

# Verify
echo ""
echo "==> Installed to ${PREFIX}"
file "${PREFIX}/lib/libopus.a"
echo ""
echo "==> Now build your Tauri app with:"
echo "    OPUS_LIB_DIR=${PREFIX}/lib LIBOPUS_STATIC=1 \\"
echo "      cargo build --manifest-path src-tauri/Cargo.toml \\"
echo "        --target x86_64-pc-windows-gnu --release"

# Clean up
rm -rf "$BUILD_DIR"
