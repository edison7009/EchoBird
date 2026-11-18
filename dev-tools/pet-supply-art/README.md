# Original TokenPet artwork

This standalone Bevy generator reproduces the original layered character art.
It has its own pinned Rust toolchain and workspace; it is not an EchoBird runtime
or build dependency. The desktop app ships the small WebP outputs only.

From this directory, render each character (`echo`, `sprout`, `twin`, `zip`),
each layer (`body`, `eyes`, `joy`, `wince`, `smile`, `open`) on both backgrounds:

```sh
cargo run -- --char sprout --layer body --bg black --out frames/sprout-body-black.png
cargo run -- --char sprout --layer body --bg white --out frames/sprout-body-white.png
```

Echo uses only body, eyes, joy and wince. With Python 3 and Pillow installed,
combine all paired renders (from the same generator revision) into aligned layers:

```sh
python3 matte.py frames layers
```

Review the output before replacing `public/pet-supply-art/`. No reference-site
artwork is distributed. Keep build caches and intermediate PNGs outside Git.
