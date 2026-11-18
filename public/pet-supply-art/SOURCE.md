# Pet supply mascot art

Original EchoBird artwork: Echo (pack), Sprout (growth), Twin (double) and
Zip (haste). Each character is modelled and rendered with Bevy 0.20 from
procedural shapes, one layer at a time (body, eyes, joy, wince, smile, open),
then cut out by rendering every layer on black and on white.

Nothing here is taken from third-party sites or packs. The reproducible generator
is in `dev-tools/pet-supply-art/` (standalone pinned Bevy/Rust workspace and `matte.py`).
See its README for rendering and layer extraction instructions.
