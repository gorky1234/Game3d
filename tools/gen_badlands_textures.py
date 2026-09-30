"""Terre rouge des Badlands dans l'atlas (nouvelle colonne d'emplacements) :
- red_sand.png : sol de terre/sable rouge-ocre (dessus du terrain) ;
- red_rock.png : strates colorées des falaises (côtés), bandes horizontales.

Dérivées des photos déjà dans l'atlas (sand.png, sandstone.png) : même grain,
mêmes normales et rugosité, seule la couleur change. L'atlas s'élargit d'une
colonne d'emplacements (544 px) : les UV sont recalculés à partir de
`meta.size` du JSON, rien d'autre à changer.

Usage : python3 tools/gen_badlands_textures.py (sauvegarde préalable dans
assets/backup_before_badlands/).
"""
import json
import numpy as np
from PIL import Image

ASSETS = "assets/"
FILES = ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png")
JSON = ASSETS + "atlas_texture.json"
TILE, MARGIN = 1024, 32
SLOT = TILE + 2 * MARGIN

meta = json.load(open(JSON))
atlases = [Image.open(ASSETS + f).convert("RGBA") for f in FILES]
w, h = atlases[0].size

# Nouvelle colonne, si elle n'existe pas déjà (script relançable).
if "red_sand.png" in meta["frames"]:
    col_x = int(meta["frames"]["red_sand.png"]["frame"]["x"])
else:
    col_x = w + MARGIN
    grown = []
    for im in atlases:
        g = Image.new("RGBA", (w + SLOT, h), (0, 0, 0, 0))
        g.paste(im, (0, 0))
        grown.append(g)
    atlases = grown
    meta["meta"]["size"]["w"] = w + SLOT


def tile(atlas, name):
    f = meta["frames"][name]["frame"]
    x, y = int(f["x"]), int(f["y"])
    return np.array(atlas.crop((x, y, x + TILE, y + TILE))).astype(np.float32)


def put(atlas, a, x, y):
    """Tuile répétable : marge = contenu enroulé."""
    padded = np.pad(np.clip(a, 0, 255).astype(np.uint8), ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="wrap")
    atlas.paste(Image.fromarray(padded, "RGBA"), (x - MARGIN, y - MARGIN))


def recolor(src, colors):
    """Garde les variations de luminosité de `src`, teinte `colors` (par pixel)."""
    luma = src[..., :3] @ np.array([0.3, 0.59, 0.11])
    rel = luma / max(luma.mean(), 1e-3)
    rel = 1.0 + (rel - 1.0) * 1.15  # grain un peu accentué
    out = colors * rel[..., None]
    return np.dstack([out, np.full((TILE, TILE), 255.0)])


rng = np.random.default_rng(7)
yy = np.arange(TILE)[:, None].repeat(TILE, 1).astype(np.float32)
xx = np.arange(TILE)[None, :].repeat(TILE, 0).astype(np.float32)

# Dessus : ocre rouge, légères taches plus claires/plus sombres (répétables).
top_color = np.array([150.0, 84.0, 58.0])
blotch = (np.sin(xx / TILE * 2 * np.pi * 2 + 1.3) * np.sin(yy / TILE * 2 * np.pi * 3 + 0.4)) * 0.08
top = recolor(tile(atlases[0], "sand.png"), top_color[None, None, :] * (1.0 + blotch[..., None]))

# Côtés : strates horizontales (la tuile couvre ~4 blocs de haut), bords
# ondulés, couleurs de terracotta/grès des badlands réels.
bands = np.array([
    [150, 80, 56], [172, 108, 74], [134, 68, 50], [186, 146, 108],
    [160, 90, 62], [124, 72, 58], [178, 122, 86], [144, 76, 54],
], dtype=np.float32)
wobble = (6.0 * np.sin(xx / TILE * 2 * np.pi * 3 + 0.7) + 3.0 * np.sin(xx / TILE * 2 * np.pi * 7 + 2.1)) * TILE / 512
v = ((yy + wobble) / TILE * len(bands)) % len(bands)
i0 = np.floor(v).astype(int) % len(bands)
i1 = (i0 + 1) % len(bands)
t = v - np.floor(v)
t = np.clip((t - 0.8) / 0.2, 0.0, 1.0)  # transitions nettes entre strates
side_colors = bands[i0] * (1 - t[..., None]) + bands[i1] * t[..., None]
side = recolor(tile(atlases[0], "sandstone.png"), side_colors)

slots = [("red_sand.png", "sand.png", top), ("red_rock.png", "sandstone.png", side)]
for k, (name, source, color) in enumerate(slots):
    x, y = col_x, MARGIN + k * SLOT
    put(atlases[0], color, x, y)
    # Normales et rugosité : celles de la tuile photo d'origine.
    put(atlases[1], tile(atlases[1], source), x, y)
    put(atlases[2], tile(atlases[2], source), x, y)
    meta["frames"][name] = {"frame": {"x": x, "y": y, "w": TILE, "h": TILE}}

for im, f in zip(atlases, FILES):
    im.save(ASSETS + f)
json.dump(meta, open(JSON, "w"), indent=2)
print("atlas", atlases[0].size, "tuiles ajoutées en x =", col_x)
