#!/usr/bin/env bash
# Builds llama.cpp's server as one self-contained binary and puts it where Tauri bundles
# sidecars: src-tauri/binaries/llama-server-<target-triple>. LocalBoard runs it in the
# background as its built-in AI; users never install anything. macOS gets Metal with the shaders
# embedded; Linux gets a portable CPU build. Windows uses fetch-llama-server.ps1 instead.
set -euo pipefail

VERSION="v0.5.0"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
OUT="$ROOT/src-tauri/binaries/llama-server-$TRIPLE"
CACHE="$ROOT/src-tauri/target/llama.cpp-$VERSION"

if [[ -x "$OUT" && "$(cat "$OUT.version" 2>/dev/null)" == "$VERSION" ]]; then
  echo "llama-server $VERSION already built"
  exit 0
fi

command -v cmake > /dev/null || { echo "cmake is required to build llama-server (macOS: brew install cmake)" >&2; exit 1; }

# Check for the sources themselves, not just the folder: a restored build cache can leave an
# empty llama.cpp folder behind.
if [[ ! -f "$CACHE/CMakeLists.txt" ]]; then
  rm -rf "$CACHE"
  mkdir -p "$CACHE"
  curl -fsSL "https://github.com/ggml-org/llama.cpp/archive/refs/tags/$VERSION.tar.gz" | tar xz -C "$CACHE" --strip-components 1
fi

if [[ "$(uname -s)" == "Darwin" ]]; then
  PLATFORM_FLAGS=(-DCMAKE_OSX_DEPLOYMENT_TARGET=11.0 -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON)
  JOBS="$(sysctl -n hw.ncpu)"
else
  PLATFORM_FLAGS=(-DGGML_METAL=OFF)
  JOBS="$(nproc)"
fi

echo "building llama-server $VERSION (first run only, takes a few minutes)"
cmake -S "$CACHE" -B "$CACHE/build" \
  -DCMAKE_BUILD_TYPE=Release \
  "${PLATFORM_FLAGS[@]}" \
  -DBUILD_SHARED_LIBS=OFF \
  -DGGML_NATIVE=OFF \
  -DLLAMA_CURL=OFF \
  -DLLAMA_OPENSSL=OFF \
  -DGGML_BLAS=OFF \
  -DLLAMA_BUILD_TESTS=OFF \
  -DLLAMA_BUILD_EXAMPLES=OFF \
  -DLLAMA_BUILD_SERVER=ON > /dev/null
cmake --build "$CACHE/build" --config Release --target llama-server -j "$JOBS" > /dev/null

mkdir -p "$(dirname "$OUT")"
cp "$CACHE/build/bin/llama-server" "$OUT"
echo "$VERSION" > "$OUT.version"
echo "built $OUT"
