#!/usr/bin/env bash
# Build the deployable browser client. All artwork/fonts are embedded in WASM.
set -Eeuo pipefail
repo_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cargo_command=${NOLAN_CARGO:-cargo}
bindgen_command=${NOLAN_WASM_BINDGEN:-wasm-bindgen}
opt_command=${NOLAN_WASM_OPT:-wasm-opt}
brotli_command=${NOLAN_BROTLI:-brotli}
for tool in "$cargo_command" "$bindgen_command" "$opt_command" "$brotli_command" sha256sum gzip; do
    command -v "$tool" >/dev/null || { echo "Required tool not found: $tool" >&2; exit 1; }
done
required=$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\([^"]*\)"$/\1/p;q;}' "$repo_root/Cargo.lock")
installed=$("$bindgen_command" --version | cut -d ' ' -f 2)
if [[ "$required" != "$installed" ]]; then
    echo "Need wasm-bindgen-cli $required; found $installed." >&2
    echo "cargo install --locked wasm-bindgen-cli --version $required" >&2
    exit 1
fi
cd "$repo_root"
# Make the artifact location deterministic even when a global Cargo config sets it.
build_target=${CARGO_TARGET_DIR:-"$repo_root/target"}
export CARGO_TARGET_DIR="$build_target"
"$cargo_command" build --locked --profile web-release -p temporal-chess-bevy -p tempfish-web --target wasm32-unknown-unknown
mkdir -p dist
stage=$(mktemp -d "$repo_root/dist/.web-build.XXXXXX")
trap 'rm -rf -- "$stage"' EXIT
"$bindgen_command" --target web --no-typescript --remove-name-section --remove-producers-section \
    --out-dir "$stage" --out-name nolan \
    "$build_target/wasm32-unknown-unknown/web-release/temporal-chess-bevy.wasm"
"$opt_command" "$stage/nolan_bg.wasm" -Os \
    --enable-multivalue --enable-mutable-globals --enable-reference-types \
    --enable-sign-ext --enable-nontrapping-float-to-int --enable-bulk-memory \
    -o "$stage/optimized.wasm"
mv "$stage/optimized.wasm" "$stage/nolan_bg.wasm"
"$bindgen_command" --target web --no-typescript --remove-name-section --remove-producers-section \
    --out-dir "$stage" --out-name tempfish "$build_target/wasm32-unknown-unknown/web-release/tempfish_web.wasm"
"$opt_command" "$stage/tempfish_bg.wasm" -O3 \
    --enable-multivalue --enable-mutable-globals --enable-reference-types \
    --enable-sign-ext --enable-nontrapping-float-to-int --enable-bulk-memory \
    -o "$stage/tempfish-optimized.wasm"
mv "$stage/tempfish-optimized.wasm" "$stage/tempfish_bg.wasm"
install -m 0644 crates/bevy_front/web/tempfish-worker.js crates/bevy_front/web/tempfish-pool.js "$stage/"
install -m 0644 crates/bevy_front/web/sound.js "$stage/sound.js"
mkdir -p "$stage/sounds"
install -m 0644 crates/bevy_front/assets/sounds/*.ogg "$stage/sounds/"
install -m 0644 crates/bevy_front/web/favicon.svg "$stage/favicon.svg"
release_id=$(cat "$stage/nolan.js" "$stage/nolan_bg.wasm" "$stage/favicon.svg" "$stage/tempfish.js" "$stage/tempfish_bg.wasm" "$stage/tempfish-worker.js" "$stage/tempfish-pool.js" "$stage/sound.js" "$stage"/sounds/*.ogg | sha256sum | cut -c 1-16)
release_dir="$repo_root/dist/web/releases/$release_id"
mkdir -p "$release_dir"
for name in nolan.js nolan_bg.wasm favicon.svg tempfish.js tempfish_bg.wasm tempfish-worker.js tempfish-pool.js sound.js; do
    gzip -9 -n -c "$stage/$name" > "$stage/$name.gz"
    "$brotli_command" --quality=11 --force --output="$stage/$name.br" "$stage/$name"
    install -m 0644 "$stage/$name" "$stage/$name.gz" "$stage/$name.br" "$release_dir/"
done
mkdir -p "$release_dir/sounds"
install -m 0644 "$stage"/sounds/*.ogg "$release_dir/sounds/"
sed "s/__NOLAN_RELEASE__/$release_id/g" crates/bevy_front/web/index.html.in > "$stage/index.html"
# Publish entrypoint last; retain older hash-addressed modules for open browser tabs.
install -m 0644 "$stage/index.html" dist/web/index.html.new
mv dist/web/index.html.new dist/web/index.html
printf 'Browser build: %s/dist/web (release %s)\n' "$repo_root" "$release_id"
wc -c "$release_dir/nolan_bg.wasm" "$release_dir/nolan_bg.wasm.br" "$release_dir/nolan_bg.wasm.gz"
