"""Double la résolution des trois atlas (couleur, normales, rugosité) : cases
de 512 -> 1024 px, marges de 16 -> 32 px. Chaque tuile garde sa place dans la
grille (coordonnées du JSON doublées) ; les tuiles sans source haute
résolution sont simplement agrandies (Lanczos), les autres sont ensuite
régénérées en natif par les scripts gen_*.py (TILE = 1024).

Normales : renormalisées après agrandissement (l'interpolation raccourcit
les vecteurs).

Usage : python3 tools/upscale_atlas_2x.py (une seule fois ; sauvegarde
préalable dans assets/backup_before_1024/).
"""
import json
import numpy as np
from PIL import Image

Image.MAX_IMAGE_PIXELS = None
ASSETS = "assets/"
JSON = ASSETS + "atlas_texture.json"

meta = json.load(open(JSON))
w, h = meta["meta"]["size"]["w"], meta["meta"]["size"]["h"]
assert w < 5000, "atlas déjà agrandi"

for name in ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png"):
    im = Image.open(ASSETS + name).convert("RGBA")
    assert im.size == (w, h)
    big = im.resize((w * 2, h * 2), Image.Resampling.LANCZOS)
    if name == "atlas_texture_normal.png":
        a = np.array(big).astype(np.float32)
        v = a[..., :3] / 127.5 - 1.0
        v /= np.maximum(np.linalg.norm(v, axis=-1, keepdims=True), 1e-4)
        a[..., :3] = (v + 1.0) * 127.5
        big = Image.fromarray(np.clip(a, 0, 255).astype(np.uint8), "RGBA")
    big.save(ASSETS + name)
    print(name, big.size)

for frame in meta["frames"].values():
    for k in ("x", "y", "w", "h"):
        frame["frame"][k] = int(frame["frame"][k]) * 2
meta["meta"]["size"] = {"w": w * 2, "h": h * 2}
json.dump(meta, open(JSON, "w"), indent=2)
