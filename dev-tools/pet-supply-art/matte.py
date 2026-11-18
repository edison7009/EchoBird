"""Turn black/white renders of each layer into transparent, aligned WebP layers.

python matte.py frames out
For every <char>-<layer>-black.png / -white.png pair, alpha comes from how much
the pixel changed between the two backgrounds; colour is recovered from the
black render. All layers of a character are cropped to one shared box, so they
stay aligned when stacked.
"""
import sys
from pathlib import Path
from PIL import Image

src, out = Path(sys.argv[1]), Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
SIZE = 384

def matte(black_path, white_path):
    b = Image.open(black_path).convert("RGB")
    w = Image.open(white_path).convert("RGB")
    bb, wb = b.getpixel((2, 2)), w.getpixel((2, 2))
    span = [max(1, wb[i] - bb[i]) for i in range(3)]
    bp, wp = b.load(), w.load()
    res = Image.new("RGBA", b.size)
    rp = res.load()
    for y in range(b.size[1]):
        for x in range(b.size[0]):
            k, v = bp[x, y], wp[x, y]
            a = 1.0 - sum((v[i] - k[i]) / span[i] for i in range(3)) / 3.0
            a = min(1.0, max(0.0, a))
            if a < 0.004:
                rp[x, y] = (0, 0, 0, 0)
                continue
            col = tuple(int(min(255, max(0, (k[i] - (1 - a) * bb[i]) / a))) for i in range(3))
            rp[x, y] = col + (int(a * 255 + 0.5),)
    return res

chars = {}
for black in sorted(src.glob("*-black.png")):
    stem = black.name[: -len("-black.png")]
    char, layer = stem.split("-", 1)
    white = src / f"{stem}-white.png"
    if white.exists():
        chars.setdefault(char, {})[layer] = matte(black, white)

for char, layers in chars.items():
    box = None
    for im in layers.values():
        bb = im.getbbox()
        if bb:
            box = bb if box is None else (min(box[0], bb[0]), min(box[1], bb[1]), max(box[2], bb[2]), max(box[3], bb[3]))
    # square box with a small margin, centred on the character
    cx, cy = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
    half = max(box[2] - box[0], box[3] - box[1]) / 2 * 1.06
    sq = (int(cx - half), int(cy - half), int(cx + half), int(cy + half))
    for layer, im in layers.items():
        im.crop(sq).resize((SIZE, SIZE), Image.LANCZOS).save(out / f"{char}-{layer}.webp", lossless=False, quality=92, method=6)
    print(char, sorted(layers), "box", sq)
