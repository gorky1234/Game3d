"""Image « imposteur » d'un houppier de feuillu (crown_impostor.png dans
l'atlas) : des dizaines de copies de la touffe photo (leaf_card.png, voir
gen_photo_foliage.py) empilées dans une silhouette ovale bosselée, plus
sombres en bas et au cœur. Les arbres lointains sont dessinés avec 3
panneaux croisés portant cette image au lieu de dizaines de touffes (voir
`tree_meshes`, src/render/tree_mesh.rs).

Usage : python3 tools/gen_crown_impostor.py (après gen_photo_foliage.py)
"""
import json, math, random
import numpy as np
from PIL import Image, ImageFilter

ATLAS, JSON = "assets/atlas_texture.png", "assets/atlas_texture.json"
TILE, MARGIN = 1024, 32
SLOT = TILE + 2 * MARGIN
POS = (5 * SLOT + MARGIN, 1 * SLOT + MARGIN)
N = 2 * TILE
rng = random.Random(9)

atlas = Image.open(ATLAS).convert("RGBA")
meta = json.load(open(JSON))
f = meta["frames"]["leaf_card.png"]["frame"]
leaf = atlas.crop((f["x"], f["y"], f["x"] + TILE, f["y"] + TILE))

img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
# Lobes de la silhouette (bosses du houppier).
lobes = [(rng.uniform(0, math.tau), rng.uniform(0.75, 1.0)) for _ in range(7)]

def radius(a):
    """Rayon relatif de la silhouette dans la direction `a` (bosselé)."""
    r = 0.78
    for la, lr in lobes:
        r = max(r, lr * max(0.0, math.cos(a - la)) ** 6)
    return r

# Du fond vers l'avant : touffes du fond sombres, du devant claires.
for k in range(170):
    depth = k / 169
    a = rng.uniform(0, math.tau)
    d = math.sqrt(rng.random()) * radius(a) * 0.82
    x = N * 0.5 + math.cos(a) * d * N * 0.46
    y = N * 0.52 + math.sin(a) * d * N * 0.4
    size = int(N * rng.uniform(0.2, 0.3))
    up = 1.0 - (y / N)  # haut plus éclairé
    light = (0.55 + 0.35 * depth) * (0.8 + 0.4 * up)
    t = leaf.resize((size, size), Image.Resampling.LANCZOS).rotate(rng.uniform(0, 360), resample=Image.Resampling.BICUBIC)
    arr = np.array(t).astype(np.float32)
    arr[..., :3] *= light
    t = Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8), "RGBA")
    img.alpha_composite(t, (int(x - size / 2), int(y - size / 2)))

small = img.resize((TILE, TILE), Image.Resampling.LANCZOS)
a = np.array(small).astype(np.float32) / 255
rgb, alpha = a[..., :3], a[..., 3:]
prem = Image.fromarray((np.concatenate([rgb * alpha, alpha], -1) * 255).astype(np.uint8), "RGBA")
blur = np.array(prem.filter(ImageFilter.GaussianBlur(10))).astype(np.float32) / 255
fill = blur[..., :3] / np.maximum(blur[..., 3:], 1e-4)
mean = (rgb * alpha).sum((0, 1)) / max(alpha.sum(), 1e-4)
fill = np.where(blur[..., 3:] > 1e-3, fill, mean)
out = np.concatenate([np.where(alpha > 0.02, rgb, fill), alpha], -1)
tile = np.clip(out * 255, 0, 255).astype(np.uint8)
padded = np.pad(tile, ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="edge")
atlas.paste(Image.fromarray(padded, "RGBA"), (POS[0] - MARGIN, POS[1] - MARGIN))
atlas.save(ATLAS)
meta["frames"]["crown_impostor.png"] = {"frame": {"x": POS[0], "y": POS[1], "w": TILE, "h": TILE}}
json.dump(meta, open(JSON, "w"), indent=2)
print("couverture", round(float((tile[..., 3] > 128).mean()), 3))
