"""Une écorce et un feuillage par essence d'arbre (voir `TreeKind::species`,
src/generation/tree_shapes.rs) : toutes les essences partageaient la même
écorce (log.png, sauf le bouleau) et le même rameau feuillu (leaf_card.png),
seule la teinte changeait. Ajoute à l'atlas (couleur, normales, rugosité) :

- bark_<essence>.png : photo d'écorce (Poly Haven, CC0), avec ses normales
  et sa rugosité (canal G de la carte « mr », relief dans R) ;
- leaves_<essence>.png : rameaux feuillus composés comme leaf_card.png (même
  disposition, voir gen_photo_foliage.py) avec les feuilles détourées d'un
  modèle Poly Haven de l'essence, puis découpés à l'octogone de la carte
  (`LEAF_CARD_OCTAGON`, src/render/tree_mesh.rs).

Les tuiles vont dans les emplacements libres, puis dans de nouvelles
colonnes à droite (relançable : les tuiles déjà présentes sont remplacées).
Sources dans tools/polyhaven/ (ignoré par git).

Usage : python3 tools/gen_tree_species.py && python3 tools/pad_atlas.py
"""
import json
import math
import random
import re
import sys

import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage

sys.argv = sys.argv[:1]
# Fonctions de composition de gen_photo_foliage.py (sans relancer la
# génération de ses tuiles).
src = open("tools/gen_photo_foliage.py").read()
exec(src[:src.index("leaves = sprites(")])

Image.MAX_IMAGE_PIXELS = None
ASSETS = "assets/"
FILES = ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png")
MARGIN = 32
SLOT = TILE + 2 * MARGIN
PH = "tools/polyhaven/"

# Écorces : essence -> photo.
BARKS = {
    "big_oak": "tree_bark_03",
    "spruce": "pine_bark",
    "willow": "bark_willow",
    "acacia": "fever_tree_bark",
    "baobab": "bark_platanus",
    "cypress": "japanese_cedar_bark",
    "giant": "metasequoia_bark",
    "palm": "palm_bark",
    "emergent": "bark_bluegum",
    "jungle": "japanese_hackberry_bark",
    "mangrove": "trident_maple_bark",
    "dead": "bark_willow_02",
    "bush": "bark_brown_01",
}
# Feuillages : essence -> (couleur, alpha, échelle des éléments sur la
# carte, saturation, gain RVB). Échelle > 1 : éléments composés (fronde,
# rameau) plutôt que feuilles isolées.
LEAVES = {
    "big_oak": ("island_tree_01_leaves_diff", "island_tree_01_leaves_alpha", 1.15, 0.75, (0.88, 1.0, 0.9)),
    "birch": ("tree_small_02_leaves_diff", "tree_small_02_leaves_alpha", 0.75, 0.85, (1.0, 1.05, 0.85)),
    "willow": ("shrub_02_Diffuse", "shrub_02_Alpha", 1.3, 0.7, (0.98, 1.05, 0.88)),
    "acacia": ("jacaranda_tree_leaves_diff", "jacaranda_tree_leaves_alpha", 1.9, 0.75, (0.95, 1.0, 0.82)),
    "baobab": ("pachira_aquatica_01_leaves_diff", "pachira_aquatica_01_leaves_alpha", 1.5, 0.7, (0.92, 1.0, 0.85)),
    "cypress": ("searsia_burchellii_Diffuse", "searsia_burchellii_Alpha", 0.8, 0.65, (0.85, 0.95, 0.8)),
    "giant": ("fir_tree_01_twig_diff", "fir_tree_01_twig_alpha", 2.0, 0.7, (0.85, 0.98, 0.9)),
    "emergent": ("island_tree_03_leaves_diff", "island_tree_03_leaves_alpha", 1.35, 0.85, (0.82, 1.0, 0.8)),
    "mangrove": ("searsia_lucida_Diffuse", "searsia_lucida_Alpha", 1.1, 0.8, (0.85, 1.0, 0.85)),
    "bush": ("shrub_04_Diffuse", "shrub_04_Alpha", 1.0, 0.75, (0.92, 1.0, 0.88)),
    "dry_bush": ("wild_rooibos_bush_Diffuse", "wild_rooibos_bush_Alpha", 1.4, 0.55, (1.05, 1.0, 0.85)),
}


def read_alpha(name, size):
    """Alpha 0..255, quel que soit le format (certains sont en entiers 32
    bits : convertis tels quels, ils saturaient tout à blanc)."""
    im = Image.open(PH + name + ".png")
    a = np.array(im).astype(np.float32)
    if a.ndim == 3:
        a = a[..., 0]
    a = a / max(float(a.max()), 1.0) * 255.0
    return np.array(Image.fromarray(a.astype(np.uint8), "L").resize(size))


def element_sprites(color_name, alpha_name):
    """Éléments détourés (feuilles, frondes, rameaux) d'une texture de
    modèle : composantes connexes de l'alpha, les plus grandes, sans les
    tiges nues (trop peu remplies) ni les zones d'écorce de la texture
    (touchant le bord, ou démesurées)."""
    color = np.array(Image.open(PH + color_name + ".png").convert("RGB"))
    a = read_alpha(alpha_name, color.shape[1::-1])
    H, W = a.shape
    # Feuilles qui se touchent (pétioles, rameaux) : séparées en coupant
    # les liaisons fines, de plus en plus fort tant que rien n'est trouvé.
    for opening in (0, 4, 8, 14):
        solid = ndimage.binary_closing(a > 60, iterations=2)
        if opening:
            solid = ndimage.binary_opening(solid, iterations=opening)
        found = components(color, a, solid, H, W)
        if found:
            break
    found.sort(key=lambda t: -t[0])
    top = found[0][0]
    return [s for area, s in found if area >= top * 0.2][:40]


def components(color, a, solid, H, W):
    lab, _ = ndimage.label(solid)
    found = []
    for i, sl in enumerate(ndimage.find_objects(lab)):
        h, w = sl[0].stop - sl[0].start, sl[1].stop - sl[1].start
        if sl[0].start <= 2 or sl[1].start <= 2 or sl[0].stop >= H - 2 or sl[1].stop >= W - 2:
            continue
        if h > H * 0.5 or w > W * 0.5:
            continue
        mask = ndimage.binary_erosion(lab[sl] == i + 1, iterations=2)
        area = mask.sum()
        if area < 1500 or area / (h * w) < 0.18:
            continue
        found.append((area, Image.fromarray(np.dstack([color[sl], np.where(mask, a[sl], 0)]).astype(np.uint8), "RGBA")))
    return found


def species_card(sprites, scale, seed):
    """Rameaux feuillus (même disposition que `leaf_card`)."""
    rng_local = random.Random(seed)
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def tint():
        return tuple(1.0 + rng_local.uniform(-0.06, 0.06) for _ in range(3))

    def branch(start, a, length, light, level):
        bend = rng_local.uniform(-0.6, 0.6)
        pts = [start]
        for i in range(1, 10):
            ang = a + bend * i / 9
            pts.append((pts[-1][0] + math.cos(ang) * length / 9, pts[-1][1] + math.sin(ang) * length / 9))
        d.line(pts, fill=(58, 46, 32, 255), width=4 if level == 0 else 2)
        for i in range(1, 10):
            t = i / 9
            ang = a + bend * t
            if level == 0 and i in (3, 5, 7) and rng_local.random() < 0.8:
                branch(pts[i], ang + rng_local.choice((-1, 1)) * rng_local.uniform(0.5, 0.9), length * rng_local.uniform(0.4, 0.6), light, 1)
            for side in (-1, 1):
                if rng_local.random() < 0.12 + 0.3 * max(0.0, scale - 1.0):
                    continue
                leaf_dir = ang + side * rng_local.uniform(0.5, 1.1)
                stamp(img, rng_local.choice(sprites), pts[i], leaf_dir + math.pi / 2, N * rng_local.uniform(0.07, 0.1) * (1 - 0.25 * t) * scale,
                      light * rng_local.uniform(0.8, 1.15), tint())
        stamp(img, rng_local.choice(sprites), pts[-1], a + bend + math.pi / 2, N * 0.09 * scale, light, tint())

    cx = cy = N * 0.5
    twigs = 22
    for k in range(twigs):
        light = 0.7 + 0.45 * k / (twigs - 1)
        a = rng_local.uniform(0, math.tau)
        r = N * rng_local.uniform(0.0, 0.2)
        branch((cx + math.cos(a + 2.5) * r, cy + math.sin(a + 2.5) * r), a, N * rng_local.uniform(0.28, 0.38), light, 0)
    return img


def octagon_mask():
    """Octogone de la carte (tree_mesh.rs), en pixels de tuile."""
    rs = open("src/render/tree_mesh.rs").read()
    body = re.search(r"const LEAF_CARD_OCTAGON: \[\[f32; 2\]; 8\] = \[(.*?)\];", rs, re.S).group(1)
    pts = [(float(a) * TILE, float(b) * TILE) for a, b in re.findall(r"\[([0-9.]+), ([0-9.]+)\]", body)]
    m = Image.new("L", (TILE, TILE), 0)
    ImageDraw.Draw(m).polygon(pts, fill=255)
    # Un peu en retrait (bords filtrés, mipmaps).
    return np.array(m).astype(np.float32) / 255


def load(name, size=TILE):
    return np.array(Image.open(PH + name + ".png").convert("RGB").resize((size, size), Image.Resampling.LANCZOS)).astype(np.float32)


meta = json.load(open(ASSETS + "atlas_texture.json"))
atlases = [Image.open(ASSETS + f).convert("RGBA") for f in FILES]
frames = meta["frames"]
w, h = meta["meta"]["size"]["w"], meta["meta"]["size"]["h"]

# Emplacements : déjà attribués, libres dans la grille, puis nouvelles
# colonnes.
names = [f"bark_{k}.png" for k in BARKS] + [f"leaves_{k}.png" for k in LEAVES]
used = {(int(f["frame"]["x"]) // SLOT, int(f["frame"]["y"]) // SLOT) for f in frames.values()}
cols, rows = w // SLOT, h // SLOT
free = [(c, r) for c in range(cols) for r in range(rows) if (c, r) not in used]
missing = [n for n in names if n not in frames]
extra = max(0, len(missing) - len(free))
new_cols = -(-extra // rows)
if new_cols:
    nw = (cols + new_cols) * SLOT
    grown = []
    for im, fill in zip(atlases, [(0, 0, 0, 0), (128, 128, 255, 255), (0, 200, 0, 255)]):
        g = Image.new("RGBA", (max(nw, w), h), fill)
        g.paste(im, (0, 0))
        grown.append(g)
    atlases = grown
    free += [(c, r) for c in range(cols, cols + new_cols) for r in range(rows)]
    meta["meta"]["size"]["w"] = max(nw, w)
for n in missing:
    c, r = free.pop(0)
    frames[n] = {"frame": {"x": c * SLOT + MARGIN, "y": r * SLOT + MARGIN, "w": TILE, "h": TILE}}


def put(atlas, a, name):
    f = frames[name]["frame"]
    padded = np.pad(np.clip(a, 0, 255).astype(np.uint8), ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="wrap" if name.startswith("bark_") else "edge")
    atlas.paste(Image.fromarray(padded, "RGBA"), (f["x"] - MARGIN, f["y"] - MARGIN))


opaque = np.full((TILE, TILE, 1), 255.0)
import os
for key, photo in (BARKS.items() if not os.environ.get("LEAVES_ONLY") else []):
    name = f"bark_{key}.png"
    color = load(photo + "_Diffuse")
    if key == "dead":
        # Bois mort : gris argenté, délavé.
        l = color @ np.array([0.3, 0.59, 0.11])
        color = l[..., None] * np.array([1.05, 1.02, 0.98]) * 1.15
    put(atlases[0], np.concatenate([color, opaque], -1), name)
    put(atlases[1], np.concatenate([load(photo + "_nor_gl"), opaque], -1), name)
    rough = load(photo + "_Rough")[..., :1]
    try:
        disp = load(photo + "_Displacement")[..., :1]
    except FileNotFoundError:
        disp = np.zeros_like(rough)
    put(atlases[2], np.concatenate([disp, rough, np.zeros_like(rough), opaque], -1), name)
    print("écorce", key, "<-", photo)

octagon = octagon_mask()
for i, (key, (col, alpha, scale, sat, gain)) in enumerate(LEAVES.items()):
    name = f"leaves_{key}.png"
    sprites = element_sprites(col, alpha)
    # Éléments plus petits tant que la carte déborde de l'octogone (bords
    # coupés net sinon).
    while True:
        card = regrade(species_card(sprites, scale, 5), saturation=sat, gain=gain)
        tile = np.array(finish(card)).astype(np.float32)
        outside = (tile[..., 3] * (1.0 - octagon)).sum() / max(tile[..., 3].sum(), 1.0)
        if outside < 0.015 or scale < 0.5:
            break
        scale *= 0.9
    tile[..., 3] *= octagon
    put(atlases[0], tile, name)
    put(atlases[1], np.concatenate([np.full((TILE, TILE, 3), 128.0) * np.array([1, 1, 2]) - np.array([0, 0, 1]), opaque], -1), name)
    put(atlases[2], np.concatenate([np.zeros((TILE, TILE, 1)), np.full((TILE, TILE, 1), 200.0), np.zeros((TILE, TILE, 1)), opaque], -1), name)
    print("feuillage", key, "<-", col, f"({len(sprites)} éléments, couverture {float((tile[..., 3] > 128).mean()):.2f})")

for im, f in zip(atlases, FILES):
    im.save(ASSETS + f)
json.dump(meta, open(ASSETS + "atlas_texture.json", "w"), indent=2)
print("atlas", atlases[0].size)
