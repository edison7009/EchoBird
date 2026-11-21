# Shared Rust pet renderer

`renderer.js` and `renderer_bg.wasm` are generated from
`src-tauri/crates/echo-supply-renderer/`. They serve both the voxel pets and
TokenPet card animation. Requirements at runtime: WebAssembly and WebGL2.

To regenerate, install Rust's `wasm32-unknown-unknown` target and
`wasm-bindgen-cli` 0.2.127, then run `npm run pet-renderer:build`.
`WASM_BINDGEN` may point to a matching CLI executable; Cargo's standard target
configuration/environment controls its build cache. No machine-specific paths
are stored in the build script. `npm run pet-renderer:check` verifies that the
packaged files match the recorded sources and hashes; it does not replace a
WASM build or native WebView testing.
