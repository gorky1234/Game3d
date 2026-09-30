"""Paroi rocheuse photo (Poly Haven rock_face_03, CC0, dans tools/polyhaven/)
dans l'atlas : rock_macro.png, avec ses cartes de normales et de rugosité.
Projetée sur ~28 blocs par le shader du terrain (voir terrain.wgsl), en plus
de la tuile rock.png répétée tous les 4 blocs : de loin, celle-ci se moyennait
en aplat uniforme (parois « dessin animé »).

Couleur désaturée et ramenée à la couleur moyenne de rock.png : la teinte
des roches (granite, calcaire, basalte...) reste celle du shader.

Usage : python3 tools/gen_rock_macro.py
"""
import json
import numpy as np
from PIL import Image

SRC = "tools/polyhaven/rock_face_03"
ASSETS = "assets/"
COLOR, NORMAL, ROUGH = (ASSETS + f for f in ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png"))
JSON = ASSETS + "atlas_texture.json"
TILE, MARGIN = 1024, 32
SLOT = TILE + 2 * MARGIN
# Seule case libre de la grille 7x6 de l'atlas.
X, Y = 5 * SLOT + MARGIN, 4 * SLOT + MARGIN
DESATURATE = 0.4

color = Image.open(COLOR).convert("RGBA")
normal = Image.open(NORMAL).convert("RGBA")
rough = Image.open(ROUGH).convert("RGBA")
meta = json.load(open(JSON))


def load(path):
    """Image 8 bits en flottants (les PNG Poly Haven sont en 16 bits)."""
    a = np.array(Image.open(path)).astype(np.float32)
    if a.max() > 255:
        a = a / 257.0
    if a.ndim == 2:
        a = np.stack([a] * 3, -1)
    im = Image.fromarray(np.clip(a[..., :3], 0, 255).astype(np.uint8), "RGB")
    return np.array(im.resize((TILE, TILE), Image.Resampling.LANCZOS)).astype(np.float32)


def put(atlas, tile_rgba, x, y):
    """Tuile répétable : marge = contenu enroulé."""
    padded = np.pad(tile_rgba, ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="wrap")
    atlas.paste(Image.fromarray(padded, "RGBA"), (x - MARGIN, y - MARGIN))


def luma(rgb):
    return rgb @ np.array([0.2126, 0.7152, 0.0722], np.float32)


f = meta["frames"]["rock.png"]["frame"]
rock = np.array(color.crop((f["x"], f["y"], f["x"] + TILE, f["y"] + TILE)))[..., :3].astype(np.float32)

c = load(f"{SRC}_diff_1k.png")
c = c + (luma(c)[..., None] - c) * DESATURATE
# Couleur moyenne (par canal) ramenée à celle de rock.png : le shader
# réchauffe la roche en supposant sa teinte gris-bleu (une photo brune
# virait à l'orange).
c *= rock.reshape(-1, 3).mean(0) / np.maximum(c.reshape(-1, 3).mean(0), 1e-3)
c = np.dstack([np.clip(c, 0, 255), np.full((TILE, TILE), 255)]).astype(np.uint8)
n = np.dstack([load(f"{SRC}_nor_gl_1k.png"), np.full((TILE, TILE), 255)]).astype(np.uint8)
r = load(f"{SRC}_rough_1k.png")[..., 0]
# Canal G : rugosité, B : métal (0), comme le reste de l'atlas.
mr = np.stack([np.full_like(r, 255), r, np.zeros_like(r), np.full_like(r, 255)], -1).astype(np.uint8)

put(color, c, X, Y)
put(normal, n, X, Y)
put(rough, mr, X, Y)
meta["frames"]["rock_macro.png"] = {"frame": {"x": X, "y": Y, "w": TILE, "h": TILE}}

color.save(COLOR)
normal.save(NORMAL)
rough.save(ROUGH)
json.dump(meta, open(JSON, "w"), indent=2)
print("rock_macro.png :", X, Y)
