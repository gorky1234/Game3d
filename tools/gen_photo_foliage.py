"""Compose les tuiles de végétation de l'atlas à partir de photos (Poly Haven,
CC0, dans tools/polyhaven/) au lieu de formes dessinées : feuilles, brins
d'herbe et frondes réels, découpés dans les atlas des modèles 3D puis
disposés en touffes (rotation, échelle, luminosité variées).

Tuiles produites : leaf_card.png (rameaux feuillus), tall_grass.png,
grass_short.png, grass_seed.png, fern.png. Affiche l'octogone de
leaf_card.png à recopier dans `LEAF_CARD_OCTAGON` (src/render/tree_mesh.rs).

Sources (CC0, https://polyhaven.com) : island_tree_02 (feuilles),
grass_medium_02 (brins), grass_bermuda_01 (touffes, tiges), fern_02.

Usage : python3 tools/gen_photo_foliage.py
"""
import json, math, random
import numpy as np
from PIL import Image, ImageDraw, ImageFilter
from scipy import ndimage

SRC = "tools/polyhaven/"
ATLAS, JSON = "assets/atlas_texture.png", "assets/atlas_texture.json"
TILE = 1024
N = 2 * TILE  # canevas de travail (suréchantillonné x2)
rng = random.Random(5)


def sprites(name, min_area, keep=lambda h, w: True):
    """Éléments (RGBA recadrés) d'un atlas : composantes connexes de l'alpha."""
    color = Image.open(f"{SRC}{name}_diff_2k.png").convert("RGB")
    alpha = Image.open(f"{SRC}{name}_alpha_2k.png").convert("L").resize(color.size)
    a = np.array(alpha)
    lab, _ = ndimage.label(ndimage.binary_closing(a > 60, iterations=2))
    out = []
    for i, sl in enumerate(ndimage.find_objects(lab)):
        h, w = sl[0].stop - sl[0].start, sl[1].stop - sl[1].start
        # Bord rogné de 2 px : les pixels de lisière de la source sont
        # clairs (liseré blanchâtre autour des brins et des feuilles).
        mask = ndimage.binary_erosion(lab[sl] == i + 1, iterations=2)
        if mask.sum() < min_area or not keep(h, w):
            continue
        rgba = np.dstack([np.array(color)[sl], np.where(mask, a[sl], 0)]).astype(np.uint8)
        out.append(Image.fromarray(rgba, "RGBA"))
    return out


def stamp(canvas, sprite, base, angle, length, light, tint=(1.0, 1.0, 1.0), stretch=1.0):
    """Pose `sprite` (pied en bas, pointe en haut) avec son pied en `base`,
    incliné de `angle` radians (0 = vertical, vers le haut), de hauteur
    `length` px, luminosité `light`."""
    w, h = sprite.size
    s = length / h
    sp = sprite.resize((max(1, int(w * s * stretch)), max(1, int(h * s))), Image.Resampling.LANCZOS)
    arr = np.array(sp).astype(np.float32)
    arr[..., :3] *= np.array(tint) * light
    sp = Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8), "RGBA")
    # Pied au centre d'un carré, pour tourner autour de lui.
    side = 2 * max(sp.size) + 2
    pad = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    pad.paste(sp, (side // 2 - sp.size[0] // 2, side // 2 - sp.size[1]))
    pad = pad.rotate(-math.degrees(angle), resample=Image.Resampling.BICUBIC)
    canvas.alpha_composite(pad, (int(base[0] - side // 2), int(base[1] - side // 2)))


def finish(canvas):
    """Réduction à la taille de tuile, couleur étendue sous le transparent
    (pas de liseré dans les mipmaps)."""
    small = canvas.resize((TILE, TILE), Image.Resampling.LANCZOS)
    a = np.array(small).astype(np.float32) / 255
    rgb, alpha = a[..., :3], a[..., 3:]
    prem = Image.fromarray((np.concatenate([rgb * alpha, alpha], -1) * 255).astype(np.uint8), "RGBA")
    blur = np.array(prem.filter(ImageFilter.GaussianBlur(10))).astype(np.float32) / 255
    fill = blur[..., :3] / np.maximum(blur[..., 3:], 1e-4)
    mean = (rgb * alpha).sum((0, 1)) / max(alpha.sum(), 1e-4)
    fill = np.where(blur[..., 3:] > 1e-3, fill, mean)
    out = np.concatenate([np.where(alpha > 0.02, rgb, fill), alpha], -1)
    return Image.fromarray((np.clip(out, 0, 1) * 255).astype(np.uint8), "RGBA")


def regrade(img, saturation, gain):
    """Désature et reteinte (les feuilles d'island_tree_02 sont olive-jaune :
    en contre-jour, les houppiers viraient à l'ocre)."""
    a = np.array(img).astype(np.float32)
    luma = a[..., :3] @ np.array([0.2126, 0.7152, 0.0722])
    a[..., :3] = (luma[..., None] + (a[..., :3] - luma[..., None]) * saturation) * np.array(gain)
    return Image.fromarray(np.clip(a, 0, 255).astype(np.uint8), "RGBA")


def jitter_tint(amount=0.06):
    return tuple(1.0 + rng.uniform(-amount, amount) for _ in range(3))


def green(sprite):
    """Élément vert (pas de brin sec couleur paille)."""
    a = np.array(sprite).astype(np.float32)
    m = a[..., 3] > 128
    r, g, b = (a[..., i][m].mean() for i in range(3))
    return g >= r * 0.97 and g > b * 1.25


leaves = sprites("island_tree_02_leaves", 50000)
blades = [b for b in sprites("grass_medium_02", 10000, keep=lambda h, w: h > 2.5 * w) if green(b)]
bermuda = sprites("grass_bermuda_01", 8000, keep=lambda h, w: h > 0.5 * w and w > 40)
fronds = sprites("fern_02", 80000)
print(f"feuilles {len(leaves)}, brins {len(blades)}, touffes {len(bermuda)}, frondes {len(fronds)}")

# --- Rameaux feuillus (feuillus, buissons) ---

def leaf_card():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def branch(start, a, length, light, level):
        bend = rng.uniform(-0.6, 0.6)
        pts = [start]
        for i in range(1, 10):
            ang = a + bend * i / 9
            pts.append((pts[-1][0] + math.cos(ang) * length / 9, pts[-1][1] + math.sin(ang) * length / 9))
        d.line(pts, fill=(58, 46, 32, 255), width=4 if level == 0 else 2)
        for i in range(1, 10):
            t = i / 9
            ang = a + bend * t
            if level == 0 and i in (3, 5, 7) and rng.random() < 0.8:
                branch(pts[i], ang + rng.choice((-1, 1)) * rng.uniform(0.5, 0.9), length * rng.uniform(0.4, 0.6), light, 1)
            for side in (-1, 1):
                if rng.random() < 0.12:
                    continue
                # Angle image (0 = vers le haut) de la feuille : le long du
                # rameau, écartée d'un côté.
                leaf_dir = ang + side * rng.uniform(0.5, 1.1)
                stamp(img, rng.choice(leaves), pts[i], leaf_dir + math.pi / 2, N * rng.uniform(0.07, 0.1) * (1 - 0.25 * t),
                      light * rng.uniform(0.8, 1.15), jitter_tint())
        stamp(img, rng.choice(leaves), pts[-1], a + bend + math.pi / 2, N * 0.09, light, jitter_tint())

    cx = cy = N * 0.5
    twigs = 22
    for k in range(twigs):
        light = 0.7 + 0.45 * k / (twigs - 1)
        a = rng.uniform(0, math.tau)
        r = N * rng.uniform(0.0, 0.2)
        branch((cx + math.cos(a + 2.5) * r, cy + math.sin(a + 2.5) * r), a, N * rng.uniform(0.28, 0.38), light, 0)
    return img

# --- Herbes ---

def grass_tile(count, min_len, max_len, spread, tufts=0, stalks=0):
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    items = []
    for _ in range(count):
        items.append(("blade", N * rng.uniform(min_len, max_len)))
    for _ in range(tufts):
        items.append(("tuft", N * rng.uniform(0.25, 0.45)))
    for _ in range(stalks):
        items.append(("stalk", N * rng.uniform(0.6, 0.95)))
    # Longs derrière (plus sombres), courts devant.
    items.sort(key=lambda it: -it[1])
    for i, (kind, length) in enumerate(items):
        x = N * (0.5 + rng.uniform(-0.5, 0.5) * spread)
        side = (x / N - 0.5) * 2
        angle = side * rng.uniform(0.05, 0.35) + rng.uniform(-0.12, 0.12)
        light = 0.75 + 0.35 * i / max(1, len(items) - 1)
        base = (x, N - 2)
        if kind == "blade":
            stamp(img, rng.choice(blades), base, angle, length, light, jitter_tint(), stretch=rng.uniform(0.5, 0.8))
        else:
            pool = [s for s in bermuda if (s.size[1] > s.size[0]) == (kind == "stalk")] or bermuda
            stamp(img, rng.choice(pool), base, angle * 0.5, length, light, jitter_tint())
    return img

# --- Fougère ---

def fern_tile():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    frond = max(fronds, key=lambda s: s.size[1])
    # Étirée en largeur : le maillage (voir `fern` dans tree_mesh.rs) donne à
    # la fronde une carte presque aussi large que longue.
    w, h = frond.size
    stamp(img, frond, (N * 0.5, N - 2), 0.0, N * 0.98, 1.0, stretch=(N * 0.8 / w) / (N * 0.98 / h))
    return img


atlas = Image.open(ATLAS).convert("RGBA")
frames = json.load(open(JSON))["frames"]
tiles = {
    "leaf_card.png": regrade(leaf_card(), saturation=0.7, gain=(0.9, 1.0, 0.92)),
    "tall_grass.png": grass_tile(80, 0.5, 0.98, 0.9, tufts=5),
    "grass_short.png": grass_tile(90, 0.3, 0.75, 0.95, tufts=9),
    "grass_seed.png": grass_tile(14, 0.4, 0.8, 0.9, stalks=16),
    "fern.png": fern_tile(),
}
for name, img in tiles.items():
    f = frames[name]["frame"]
    tile = finish(img)
    padded = np.pad(np.array(tile), ((16, 16), (16, 16), (0, 0)), mode="edge")
    atlas.paste(Image.fromarray(padded, "RGBA"), (int(f["x"]) - 16, int(f["y"]) - 16))
    if name == "leaf_card.png":
        ys, xs = np.nonzero(np.array(tile)[..., 3] > 40)
        u, v = (xs + 0.5) / TILE, (ys + 0.5) / TILE
        smin, smax, dmin, dmax = (u + v).min(), (u + v).max(), (u - v).min(), (u - v).max()
        pts = [(smin - v.min(), v.min()), (dmax + v.min(), v.min()), (u.max(), u.max() - dmax), (u.max(), smax - u.max()),
               (smax - v.max(), v.max()), (dmin + v.max(), v.max()), (u.min(), u.min() - dmin), (u.min(), smin - u.min())]
        print("coverage", round(float((np.array(tile)[..., 3] > 128).mean()), 3))
        print("const LEAF_CARD_OCTAGON: [[f32; 2]; 8] = [" + ", ".join(f"[{p[0]:.3f}, {p[1]:.3f}]" for p in pts) + "];")
atlas.save(ATLAS)
