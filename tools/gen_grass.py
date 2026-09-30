"""Régénère les tuiles d'herbe de l'atlas (tall_grass, grass_short, grass_seed)
avec des brins lisses (suréchantillonnage x4), un dégradé pied sombre -> pointe
claire et plus jaune, et une couleur étendue sous les zones transparentes (pas
de liseré sombre dans les mipmaps).

Usage : python3 tools/gen_grass.py assets/atlas_texture.png assets/atlas_texture.json
"""
import json, math, random, sys
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ATLAS = sys.argv[1]
JSON = sys.argv[2]
SS = 4
N = 1024 * SS

def blade_poly(x0, length, width, bend, lean, steps=24):
    """Contour d'un brin effilé et courbé. Pied en (x0, N), pointe en haut."""
    left, right, ts = [], [], []
    for i in range(steps + 1):
        t = i / steps
        # Courbure : déport latéral croissant (quadratique) + inclinaison.
        x = x0 + lean * t * length + bend * t * t * length
        y = N - t * length
        dx = lean * length + 2 * bend * t * length
        dy = -length
        l = math.hypot(dx, dy)
        nx, ny = -dy / l, dx / l
        w = width * (1 - t) ** 0.8 * 0.5
        left.append((x + nx * w, y + ny * w))
        right.append((x - nx * w, y - ny * w))
        ts.append(t)
    return left, right, ts

def draw_blade(img, x0, length, width, bend, lean, base_col, tip_col, fold=True):
    left, right, ts = blade_poly(x0, length, width, bend, lean)
    d = ImageDraw.Draw(img)
    for i in range(len(ts) - 1):
        t = (ts[i] + ts[i + 1]) / 2
        k = t ** 0.9
        c = tuple(int(255 * (b + (tp - b) * k)) for b, tp in zip(base_col, tip_col))
        # Deux moitiés (nervure) : une face un peu plus claire, effet de pli.
        mid_a = ((left[i][0] + right[i][0]) / 2, (left[i][1] + right[i][1]) / 2)
        mid_b = ((left[i + 1][0] + right[i + 1][0]) / 2, (left[i + 1][1] + right[i + 1][1]) / 2)
        light = tuple(min(255, int(v * 1.12)) for v in c) if fold else c
        d.polygon([left[i], left[i + 1], mid_b, mid_a], fill=light + (255,))
        d.polygon([mid_a, mid_b, right[i + 1], right[i]], fill=c + (255,))
    return (left[-1][0] + right[-1][0]) / 2, (left[-1][1] + right[-1][1]) / 2

def jitter(col, rng, amount=0.08):
    s = 1 + rng.uniform(-amount, amount)
    h = rng.uniform(-0.03, 0.03)
    return (col[0] * s + h, col[1] * s, col[2] * s - h)

def grass_tile(rng, count, min_len, max_len, width, base, tip, spread=0.9):
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    blades = []
    for _ in range(count):
        x0 = N * (0.5 + rng.uniform(-0.5, 0.5) * spread)
        length = N * rng.uniform(min_len, max_len)
        # Brins du bord penchés vers l'extérieur (touffe en éventail).
        side = (x0 / N - 0.5) * 2
        lean = side * rng.uniform(0.05, 0.3) + rng.uniform(-0.08, 0.08)
        bend = rng.uniform(-0.25, 0.25)
        # La pointe doit rester dans la tuile.
        tip_x = x0 + (lean + bend) * length
        if not (0.03 * N < tip_x < 0.97 * N):
            lean = -lean
        blades.append((length, x0, width * N * rng.uniform(0.7, 1.3), bend, lean))
    # Longs brins derrière, courts devant.
    blades.sort(key=lambda b: -b[0])
    for length, x0, w, bend, lean in blades:
        # Brins de derrière plus sombres (ombre dans la touffe).
        b = jitter(base, rng)
        t = jitter(tip, rng)
        draw_blade(img, x0, length, w, bend, lean, b, t)
    return img

def seed_tile(rng):
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # Quelques brins fins à la base.
    base = grass_tile(rng, 14, 0.25, 0.5, 0.022, (0.16, 0.22, 0.1), (0.42, 0.5, 0.24))
    for _ in range(26):
        x0 = N * rng.uniform(0.08, 0.92)
        length = N * rng.uniform(0.6, 0.95)
        lean = (x0 / N - 0.5) * rng.uniform(0.05, 0.25)
        bend = rng.uniform(-0.12, 0.12)
        if not (0.05 * N < x0 + (lean + bend) * length < 0.95 * N):
            lean = -lean
        tx, ty = draw_blade(img, x0, length, 0.012 * N, bend, lean, (0.2, 0.25, 0.12), (0.55, 0.52, 0.3), fold=False)
        # Épi : chapelet de grains le long du haut de la tige.
        n = rng.randint(11, 15)
        dirx = (lean + 2 * bend) * length
        dl = math.hypot(dirx, length)
        ux, uy = dirx / dl, -length / dl
        for k in range(n):
            s = k / n * 0.15 * N
            cx, cy = tx - ux * s, ty - uy * s
            r = 0.0075 * N * (1 - 0.3 * k / n) * rng.uniform(0.8, 1.2)
            off = (1 if k % 2 else -1) * r * 0.7
            col = jitter((0.78, 0.66, 0.4), rng, 0.1)
            c = tuple(int(255 * min(1, v)) for v in col) + (255,)
            d.ellipse([cx + off - r, cy - r * 2.0, cx + off + r, cy + r * 2.0], fill=c)
    base.alpha_composite(img)
    return base

def finish(img):
    small = img.resize((512, 512), Image.Resampling.LANCZOS)
    a = np.array(small).astype(np.float32) / 255
    rgb, alpha = a[..., :3], a[..., 3:]
    # Couleur des pixels transparents : moyenne floue des pixels opaques
    # voisins (pas de liseré noir au filtrage bilinéaire / mipmaps).
    prem = Image.fromarray((np.concatenate([rgb * alpha, alpha], -1) * 255).astype(np.uint8), "RGBA")
    out_rgb = rgb.copy()
    fill = None
    for radius in (2, 6, 16, 48):
        blur = np.array(prem.filter(ImageFilter.GaussianBlur(radius))).astype(np.float32) / 255
        c = blur[..., :3] / np.maximum(blur[..., 3:], 1e-4)
        mask = (blur[..., 3:] > 1e-3)
        fill = np.where(mask, c, fill if fill is not None else c)
        if fill is not None and radius == 2:
            first = fill
    fill = np.where(alpha > 0.5, rgb, fill)
    out = np.concatenate([np.where(alpha > 0.02, rgb, fill), alpha], -1)
    return Image.fromarray((np.clip(out, 0, 1) * 255).astype(np.uint8), "RGBA")

rng = random.Random(7)
tiles = {
    # Couleurs proches des anciennes tuiles (même luminosité moyenne) : c'est
    # la couleur de sommet qui teinte (voir `plant_mesh`).
    "tall_grass.png": grass_tile(rng, 75, 0.5, 0.98, 0.03, (0.17, 0.24, 0.1), (0.52, 0.6, 0.3)),
    "grass_short.png": grass_tile(rng, 95, 0.35, 0.8, 0.028, (0.16, 0.22, 0.09), (0.47, 0.55, 0.26)),
    "grass_seed.png": seed_tile(rng),
}
atlas = Image.open(ATLAS).convert("RGBA")
frames = json.load(open(JSON))["frames"]
for name, img in tiles.items():
    f = frames[name]["frame"]
    tile = finish(img)
    # Efface puis remplace la tuile (marges de l'atlas intactes).
    atlas.paste(tile, (int(f["x"]), int(f["y"])))
atlas.save(ATLAS)
