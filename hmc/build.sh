#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="$ROOT_DIR/build"
EXTERNALS_DIR="$ROOT_DIR/externals"
SDL_SRC_DIR="$EXTERNALS_DIR/SDL"
SDL_BUILD_DIR="$BUILD_DIR/sdl"
SDL_TAG="release-3.4.8"
GAME_SO_TMP="$BUILD_DIR/handmade_building.so"

COMMON_COMPILER_FLAGS=(
    -std=gnu11
    -g
    -O0
    -Wall
    -Wextra
    -Werror
    -Wno-unused-parameter
    -Wno-unused-variable
    -Wno-unused-but-set-variable
    -Wno-unused-function
    -Wno-sign-compare
    -DDEBUG_PROFILE=1
    -DDEBUG_ASSERTIONS=1
    -DHANDMADE_WIN32=0
    -DHANDMADE_LINUX=1
    -I"$ROOT_DIR/src"
)

SDL_COMPILER_FLAGS=(
    -I"$SDL_SRC_DIR/include"
    -I"$SDL_BUILD_DIR/include-revision"
    -I"$SDL_BUILD_DIR/include-config-debug"
)

mkdir -p "$BUILD_DIR" "$EXTERNALS_DIR"

if [ ! -d "$SDL_SRC_DIR/.git" ]; then
    git clone --depth 1 --branch "$SDL_TAG" "https://github.com/libsdl-org/SDL.git" "$SDL_SRC_DIR"
fi

cmake -S "$SDL_SRC_DIR" -B "$SDL_BUILD_DIR" -G Ninja \
    -DCMAKE_BUILD_TYPE=Debug \
    -DSDL_SHARED=OFF \
    -DSDL_STATIC=ON \
    -DSDL_TEST_LIBRARY=OFF \
    -DSDL_TESTS=OFF \
    -DSDL_EXAMPLES=OFF \
    -DSDL_INSTALL_TESTS=OFF \
    -DSDL_X11_XSCRNSAVER=OFF

cmake --build "$SDL_BUILD_DIR" --target SDL3-static

cat >"$ROOT_DIR/compile_commands.json" <<EOF
[
  {
    "directory": "$ROOT_DIR",
    "arguments": [
      "gcc",
      "-std=gnu11",
      "-g",
      "-O0",
      "-Wall",
      "-Wextra",
      "-Werror",
      "-Wno-unused-parameter",
      "-Wno-unused-variable",
      "-Wno-unused-but-set-variable",
      "-Wno-unused-function",
      "-Wno-sign-compare",
      "-DDEBUG_PROFILE=1",
      "-DDEBUG_ASSERTIONS=1",
      "-DHANDMADE_WIN32=0",
      "-DHANDMADE_LINUX=1",
      "-I./src",
      "-fPIC",
      "-c",
      "./src/handmade.c",
      "-o",
      "./build/handmade.o"
    ],
    "file": "./src/handmade.c"
  },
  {
    "directory": "$ROOT_DIR",
    "arguments": [
      "gcc",
      "-std=gnu11",
      "-g",
      "-O0",
      "-Wall",
      "-Wextra",
      "-Werror",
      "-Wno-unused-parameter",
      "-Wno-unused-variable",
      "-Wno-unused-but-set-variable",
      "-Wno-unused-function",
      "-Wno-sign-compare",
      "-DDEBUG_PROFILE=1",
      "-DDEBUG_ASSERTIONS=1",
      "-DHANDMADE_WIN32=0",
      "-DHANDMADE_LINUX=1",
      "-I./src",
      "-I./externals/SDL/include",
      "-I./build/sdl/include-revision",
      "-I./build/sdl/include-config-debug",
      "-c",
      "./src/linux_sdl_handmade.c",
      "-o",
      "./build/linux_sdl_handmade.o"
    ],
    "file": "./src/linux_sdl_handmade.c"
  }
]
EOF

gcc "${COMMON_COMPILER_FLAGS[@]}" \
    -fPIC -shared "$ROOT_DIR/src/handmade.c" \
    -o "$GAME_SO_TMP"
mv "$GAME_SO_TMP" "$BUILD_DIR/handmade.so"

gcc "${COMMON_COMPILER_FLAGS[@]}" "${SDL_COMPILER_FLAGS[@]}" \
    "$ROOT_DIR/src/linux_sdl_handmade.c" \
    -o "$BUILD_DIR/handmade" \
    -Wl,--whole-archive "$SDL_BUILD_DIR/libSDL3.a" -Wl,--no-whole-archive \
    -ldl -lm -pthread
