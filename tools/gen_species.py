"""Tuiles de variété pour l'atlas (emplacements libres de la dernière rangée
et de la dernière colonne) :

- plantes de prairie, en plus de l'herbe haute et de la graminée à épis
  (voir `plant_mesh`, src/render/plant_mesh.rs) : clover.png (trèfle et
  feuilles larges basses), thistle.png (chardon), dry_grass.png (herbe sèche
  couchée), yarrow.png (achillée, ombelles blanches) ;
- imposteurs d'arbres lointains (voir `tree_meshes`, src/render/tree_mesh.rs) :
  crown_impostor_2.png (houppier haut et ovale), crown_impostor_3.png
  (houppier large et aplati), pine_impostor.png (sapin conique).

Réutilise les découpes de photos de gen_photo_foliage.py (Poly Haven, CC0) et
les tuiles déjà dans l'atlas (leaf_card.png, pine_card.png).

Usage : python3 tools/gen_species.py (après gen_photo_foliage.py)
"""
import json, math, random, sys
import numpy as np
from PIL import Image, ImageDraw

sys.argv = sys.argv[:1]
# Fonctions de découpe et de pose des photos (sans relancer la génération
# des tuiles de gen_photo_foliage.py : seules ses définitions sont reprises).
src = open("tools/gen_photo_foliage.py").read()
exec(src[:src.index("atlas = Image.open(ATLAS)")])
rng = random.Random(23)

TILE, MARGIN = 1024, 32
SLOT = TILE + 2 * MARGIN
atlas = Image.open(ATLAS).convert("RGBA")
meta = json.load(open(JSON))


def slot(col, row):
    return (col * SLOT + MARGIN, row * SLOT + MARGIN)


def atlas_tile(name):
    f = meta["frames"][name]["frame"]
    return atlas.crop((f["x"], f["y"], f["x"] + f["w"], f["y"] + f["h"]))


def tinted(sprite, gain):
    a = np.array(sprite).astype(np.float32)
    a[..., :3] *= np.array(gain)
    return Image.fromarray(np.clip(a, 0, 255).astype(np.uint8), "RGBA")


# --- Trèfle et feuilles larges basses (plantain) ---

def clover_tile():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # Rosettes de feuilles larges (plantain), posées bas, presque couchées.
    for _ in range(4):
        cx = N * rng.uniform(0.2, 0.8)
        for k in range(7):
            a = rng.uniform(-1.3, 1.3)
            stamp(img, rng.choice(leaves), (cx, N - 2), a, N * rng.uniform(0.25, 0.4), rng.uniform(0.7, 1.0),
                  (0.85, 1.0, 0.8), stretch=rng.uniform(0.7, 1.0))
    # Trèfles : tige courte, trois folioles (petites feuilles photo arrondies
    # par un étirement en largeur) disposées en étoile.
    for _ in range(46):
        x = N * rng.uniform(0.05, 0.95)
        h = N * rng.uniform(0.12, 0.38)
        lean = rng.uniform(-0.3, 0.3)
        top = (x + lean * h, N - 2 - h)
        d.line([(x, N - 2), top], fill=(78, 98, 52, 255), width=4)
        light = rng.uniform(0.7, 1.05)
        size = N * rng.uniform(0.05, 0.075)
        start = rng.uniform(0, math.tau)
        for k in range(3):
            a = start + k * math.tau / 3
            stamp(img, rng.choice(leaves), top, a, size, light, (0.8, 1.0, 0.75), stretch=1.6)
        d = ImageDraw.Draw(img)
    # Quelques fleurs de trèfle blanc rosé.
    for _ in range(6):
        x, y = N * rng.uniform(0.1, 0.9), N * rng.uniform(0.5, 0.7)
        d.line([(x, N - 2), (x, y)], fill=(80, 100, 55, 255), width=4)
        for _ in range(14):
            fx, fy = x + rng.uniform(-14, 14), y + rng.uniform(-16, 8)
            d.ellipse([fx - 5, fy - 5, fx + 5, fy + 5], fill=(232, int(rng.uniform(205, 225)), 210, 255))
    return img


# --- Chardon ---

def thistle_tile():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    stems = []
    for k in range(3):
        x = N * (0.35 + 0.15 * k + rng.uniform(-0.05, 0.05))
        top = (x + rng.uniform(-60, 60), N * rng.uniform(0.08, 0.3))
        stems.append((x, top))
    for x, top in stems:
        d.line([(x, N - 2), top], fill=(92, 105, 80, 255), width=9)
        # Feuilles épineuses : feuilles photo étroites, gris-vert, en épi.
        for i in range(7):
            t = 0.15 + 0.12 * i
            p = (x + (top[0] - x) * t, N - 2 + (top[1] - N + 2) * t)
            for side in (-1, 1):
                stamp(img, rng.choice(leaves), p, side * rng.uniform(0.9, 1.4), N * (0.2 - 0.02 * i), rng.uniform(0.75, 0.95),
                      (0.85, 0.95, 0.85), stretch=0.45)
    d = ImageDraw.Draw(img)
    for x, top in stems:
        # Capitule : bulbe épineux vert, houppe pourpre.
        cx, cy = top
        d.ellipse([cx - 26, cy - 10, cx + 26, cy + 42], fill=(90, 110, 70, 255))
        for k in range(26):
            a = -math.pi / 2 + rng.uniform(-1.2, 1.2)
            l = rng.uniform(26, 48)
            d.line([(cx, cy + 4), (cx + math.cos(a) * l, cy + 4 + math.sin(a) * l)],
                   fill=(int(rng.uniform(140, 175)), int(rng.uniform(55, 80)), int(rng.uniform(140, 170)), 255), width=5)
    return img


# --- Herbe sèche couchée ---

def dry_grass_tile():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    items = sorted([N * rng.uniform(0.45, 1.0) for _ in range(70)], reverse=True)
    for i, length in enumerate(items):
        x = N * rng.uniform(0.02, 0.98)
        # Brins fortement inclinés, des deux côtés : herbe versée par le vent
        # et la pluie.
        side = 1 if rng.random() < 0.65 else -1
        angle = side * rng.uniform(0.55, 1.25)
        light = 0.8 + 0.3 * i / len(items)
        straw = (1.25 + rng.uniform(-0.1, 0.1), 1.0, 0.55)
        stamp(img, rng.choice(blades), (x, N - 2), angle, length * 0.8, light, straw, stretch=rng.uniform(0.5, 0.8))
    # Quelques brins encore verts dressés.
    for _ in range(10):
        stamp(img, rng.choice(blades), (N * rng.uniform(0.1, 0.9), N - 2), rng.uniform(-0.2, 0.2), N * rng.uniform(0.3, 0.55), 0.9)
    return regrade(img, 0.75, (1.0, 1.0, 1.0))


# --- Achillée ---

def yarrow_tile():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    for _ in range(18):
        stamp(img, rng.choice(blades), (N * rng.uniform(0.15, 0.85), N - 2), rng.uniform(-0.3, 0.3), N * rng.uniform(0.25, 0.45), 0.85,
              stretch=0.5)
    d = ImageDraw.Draw(img)
    for k in range(5):
        x = N * rng.uniform(0.15, 0.85)
        top = (x + rng.uniform(-50, 50), N * rng.uniform(0.12, 0.35))
        d.line([(x, N - 2), top], fill=(95, 115, 70, 255), width=6)
        # Ombelle : amas plat de petites fleurs blanc cassé.
        w = rng.uniform(55, 85)
        for _ in range(60):
            fx = top[0] + rng.uniform(-w, w)
            fy = top[1] + rng.uniform(-12, 12) + abs(fx - top[0]) * 0.15
            r = rng.uniform(6, 10)
            v = rng.uniform(210, 240)
            d.ellipse([fx - r, fy - r, fx + r, fy + r], fill=(int(v), int(v * 0.97), int(v * 0.9), 255))
    return img


# --- Imposteurs ---

def crown(aspect, flat, lobes_count, seed):
    """Houppier vu de côté : touffes de leaf_card.png empilées dans une
    silhouette bosselée d'`aspect` (hauteur / largeur), base aplatie `flat`."""
    r = random.Random(seed)
    leaf = atlas_tile("leaf_card.png")
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    lobes = [(r.uniform(0, math.tau), r.uniform(0.7, 1.0)) for _ in range(lobes_count)]

    def radius(a):
        v = 0.72
        for la, lr in lobes:
            v = max(v, lr * max(0.0, math.cos(a - la)) ** 5)
        return v

    for k in range(190):
        depth = k / 189
        a = r.uniform(0, math.tau)
        dist = math.sqrt(r.random()) * radius(a) * 0.82
        x = N * 0.5 + math.cos(a) * dist * N * 0.46
        dy = math.sin(a) * dist * N * 0.46 * aspect
        if dy > 0:
            dy *= 1.0 - flat
        y = N * 0.5 + dy
        size = int(N * r.uniform(0.18, 0.28))
        up = 1.0 - y / N
        light = (0.55 + 0.35 * depth) * (0.8 + 0.4 * up)
        t = leaf.resize((size, size), Image.Resampling.LANCZOS).rotate(r.uniform(0, 360), resample=Image.Resampling.BICUBIC)
        img.alpha_composite(tinted(t, (light, light, light)), (int(x - size / 2), int(y - size / 2)))
    return img


def pine_impostor():
    """Sapin vu de côté : étages de branches (pine_card.png) du plus large en
    bas au plus étroit en haut, qui se chevauchent au centre (pas de vide le
    long du tronc) ; pointe fine."""
    card = atlas_tile("pine_card.png")
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.line([(N * 0.5, N - 2), (N * 0.5, N * 0.04)], fill=(55, 42, 32, 255), width=12)
    tiers = 22
    for i in range(tiers):
        t = i / (tiers - 1)
        y = N * (0.92 - 0.84 * t)
        # Étages de longueur irrégulière : silhouette dentelée, pas un cône lisse.
        half = N * (0.42 * (1 - t) ** 0.9 + 0.035) * rng.uniform(0.72, 1.12)
        light = 0.65 + 0.5 * t
        # Masse centrée sur le tronc (sinon une raie claire au milieu).
        w = int(half * 1.3)
        b = card.resize((w, int(max(28, w * 0.5))), Image.Resampling.LANCZOS)
        img.alpha_composite(tinted(b, (light * 0.85,) * 3), (int(N * 0.5 - w / 2), int(y - b.size[1] * 0.5)))
        for side in (-1, 1):
            for k in range(4):
                length = half * rng.uniform(0.8, 1.1) + N * 0.04
                w = int(length)
                h = int(max(28, length * 0.55))
                b = card.resize((w, h), Image.Resampling.LANCZOS)
                if side < 0:
                    b = b.transpose(Image.Transpose.FLIP_LEFT_RIGHT)
                b = b.rotate(-side * rng.uniform(10, 25), resample=Image.Resampling.BICUBIC, expand=True)
                overlap = N * 0.03
                x = N * 0.5 - overlap if side > 0 else N * 0.5 + overlap - b.size[0]
                img.alpha_composite(tinted(b, (light, light, light)), (int(x), int(y - b.size[1] * 0.45 + rng.uniform(-10, 10))))
    return img


tiles = {
    "clover.png": (clover_tile(), slot(0, 5)),
    "thistle.png": (thistle_tile(), slot(1, 5)),
    "dry_grass.png": (dry_grass_tile(), slot(2, 5)),
    "yarrow.png": (yarrow_tile(), slot(3, 5)),
    "crown_impostor_2.png": (crown(1.3, 0.15, 6, 41), slot(4, 5)),
    "crown_impostor_3.png": (crown(0.7, 0.45, 9, 42), slot(5, 5)),
    "pine_impostor.png": (pine_impostor(), slot(5, 3)),
}
for name, (img, pos) in tiles.items():
    tile = finish(img)
    padded = np.pad(np.array(tile), ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="edge")
    atlas.paste(Image.fromarray(padded, "RGBA"), (pos[0] - MARGIN, pos[1] - MARGIN))
    meta["frames"][name] = {"frame": {"x": pos[0], "y": pos[1], "w": TILE, "h": TILE}}
    print(name, "couverture", round(float((np.array(tile)[..., 3] > 128).mean()), 3))
atlas.save(ATLAS)
json.dump(meta, open(JSON, "w"), indent=2)
