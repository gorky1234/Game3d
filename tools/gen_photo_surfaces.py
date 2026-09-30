"""Surfaces photo (Poly Haven, CC0, dans tools/polyhaven/) dans l'atlas,
avec leurs cartes de normales et de rugosité :
- log.png : écorce de chêne (jolcham_oak_bark_01) ;
- boulder.png (nouvel emplacement) : roche à lichens (lichen_rock), pour les
  rochers posés au sol (voir `rock` dans src/render/tree_mesh.rs).
Adoucit aussi les marques noires de birch.png (trop graphiques).

Usage : python3 tools/gen_photo_surfaces.py [--no-birch]
"""
import json
import sys
import numpy as np
from PIL import Image

SRC = "tools/polyhaven/"
ASSETS = "assets/"
COLOR, NORMAL, ROUGH = (ASSETS + f for f in ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png"))
JSON = ASSETS + "atlas_texture.json"
TILE, MARGIN = 1024, 32
SLOT = TILE + 2 * MARGIN

color = Image.open(COLOR).convert("RGBA")
normal = Image.open(NORMAL).convert("RGBA")
rough = Image.open(ROUGH).convert("RGBA")
meta = json.load(open(JSON))


def load(path, mode):
    """Image 8 bits (les PNG Poly Haven peuvent être en 16 bits)."""
    im = Image.open(path)
    a = np.array(im).astype(np.float32)
    if a.max() > 255:
        a = a / 257.0
    if a.ndim == 2:
        a = np.stack([a] * 3, -1)
    return Image.fromarray(np.clip(a[..., :3], 0, 255).astype(np.uint8), "RGB").resize((TILE, TILE), Image.Resampling.LANCZOS)


def put(atlas, tile_rgba, x, y):
    """Tuile répétable : marge = contenu enroulé."""
    a = np.array(tile_rgba)
    padded = np.pad(a, ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="wrap")
    atlas.paste(Image.fromarray(padded, "RGBA"), (x - MARGIN, y - MARGIN))


def surface(name, x, y, gain=(1.0, 1.0, 1.0)):
    c = np.array(load(f"{SRC}{name}_diff_1k.png", "RGB")).astype(np.float32) * np.array(gain)
    c = np.dstack([np.clip(c, 0, 255), np.full((TILE, TILE), 255)]).astype(np.uint8)
    n = np.array(load(f"{SRC}{name}_nor_gl_1k.png", "RGB"))
    n = np.dstack([n, np.full((TILE, TILE), 255, np.uint8)])
    r = np.array(load(f"{SRC}{name}_rough_1k.png", "L"))[..., 0]
    # Canal G : rugosité, B : métal (0), comme le reste de l'atlas.
    mr = np.stack([np.full_like(r, 255), r, np.zeros_like(r), np.full_like(r, 255)], -1)
    put(color, Image.fromarray(c, "RGBA"), x, y)
    put(normal, Image.fromarray(n, "RGBA"), x, y)
    put(rough, Image.fromarray(mr, "RGBA"), x, y)


f = meta["frames"]["log.png"]["frame"]
surface("jolcham_oak_bark_01", int(f["x"]), int(f["y"]))

bx, by = 5 * SLOT + MARGIN, 2 * SLOT + MARGIN
surface("lichen_rock", bx, by)
meta["frames"]["boulder.png"] = {"frame": {"x": bx, "y": by, "w": TILE, "h": TILE}}

# Bouleau : marques sombres ramenées vers un gris-brun, moins contrastées.
# Pas idempotent (atténuerait encore) : `--no-birch` pour relancer le
# script sur un atlas où c'est déjà fait.
if "--no-birch" not in sys.argv:
    f = meta["frames"]["birch.png"]["frame"]
    box = (int(f["x"]), int(f["y"]), int(f["x"]) + TILE, int(f["y"]) + TILE)
    b = np.array(color.crop(box)).astype(np.float32)
    luma = b[..., :3] @ np.array([0.2126, 0.7152, 0.0722]) / 255
    dark = np.clip((0.55 - luma) / 0.45, 0, 1)[..., None]
    b[..., :3] = b[..., :3] * (1 - 0.55 * dark) + np.array([92, 84, 74]) * 0.55 * dark
    put(color, Image.fromarray(np.clip(b, 0, 255).astype(np.uint8), "RGBA"), box[0], box[1])

color.save(COLOR)
normal.save(NORMAL)
rough.save(ROUGH)
json.dump(meta, open(JSON, "w"), indent=2)
print("ok")
