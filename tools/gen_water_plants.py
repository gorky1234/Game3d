"""Plantes aquatiques et marines dans l'atlas (colonne ajoutée par
gen_badlands_textures.py, emplacements libres sous red_rock.png) :
- reed.png : touffe de roseaux et massettes (vue de côté) ;
- lily_pad.png : feuilles de nénuphar et une fleur (vue de dessus) ;
- kelp.png : fronde de varech (lame ondulée le long d'un stipe) ;
- coral.png : corail branchu clair (la couleur vient de la teinte des sommets).

Brins suréchantillonnés (x4), couleur étendue sous les zones transparentes
(pas de liseré sombre dans les mipmaps), normale plate et rugosité moyenne
dans les cartes associées (alpha identique).

Usage : python3 tools/gen_water_plants.py
"""
import json, math, random
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ASSETS = "assets/"
FILES = ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png")
JSON = ASSETS + "atlas_texture.json"
TILE, MARGIN = 1024, 32
SLOT = TILE + 2 * MARGIN
SS = 4
N = TILE * SS

meta = json.load(open(JSON))
atlases = [Image.open(ASSETS + f).convert("RGBA") for f in FILES]
col_x = int(meta["frames"]["red_sand.png"]["frame"]["x"])


def rgb(c, a=255):
    return tuple(int(255 * min(1.0, max(0.0, v))) for v in c) + (a,)


def lerp(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))


def blade(d, x0, y0, length, width, bend, lean, base, tip, taper=0.8, steps=28):
    """Brin effilé de (x0, y0) vers le haut."""
    left, right = [], []
    for i in range(steps + 1):
        t = i / steps
        x = x0 + lean * t * length + bend * t * t * length
        y = y0 - t * length
        dx, dy = lean * length + 2 * bend * t * length, -length
        l = math.hypot(dx, dy)
        nx, ny = -dy / l, dx / l
        w = width * (1 - t) ** taper * 0.5
        left.append((x + nx * w, y + ny * w))
        right.append((x - nx * w, y - ny * w))
    for i in range(steps):
        c = lerp(base, tip, ((i + 0.5) / steps) ** 0.9)
        d.polygon([left[i], left[i + 1], right[i + 1], right[i]], fill=rgb(c))
    return left[-1], right[-1]


def reed(rng):
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    stems = []
    for _ in range(34):
        x0 = N * rng.uniform(0.15, 0.85)
        length = N * rng.uniform(0.55, 0.97)
        lean = (x0 / N - 0.5) * rng.uniform(0.0, 0.25) + rng.uniform(-0.04, 0.04)
        bend = rng.uniform(-0.12, 0.12)
        stems.append((length, x0, lean, bend))
    stems.sort(key=lambda s: -s[0])
    for length, x0, lean, bend in stems:
        base = (0.2 + rng.uniform(-0.03, 0.03), 0.28, 0.12)
        tip = (0.55 + rng.uniform(-0.05, 0.05), 0.58, 0.3)
        blade(d, x0, N, length, N * rng.uniform(0.018, 0.03), bend, lean, base, tip)
    # Massettes : épis bruns cylindriques en haut de tiges droites.
    for _ in range(5):
        x0 = N * rng.uniform(0.3, 0.7)
        length = N * rng.uniform(0.7, 0.9)
        lean = rng.uniform(-0.05, 0.05)
        blade(d, x0, N, length, N * 0.012, 0.0, lean, (0.25, 0.3, 0.14), (0.45, 0.45, 0.25), taper=0.2)
        cx = x0 + lean * length * 0.82
        cy = N - length * 0.82
        w, h = N * 0.018, N * 0.075
        d.rounded_rectangle([cx - w, cy - h, cx + w, cy + h], radius=w, fill=rgb((0.36, 0.23, 0.12)))
    return img


def lily(rng):
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pads = [(0.32, 0.36, 0.26), (0.7, 0.3, 0.2), (0.62, 0.7, 0.27), (0.25, 0.75, 0.17)]
    for cx, cy, r in pads:
        cx, cy, r = cx * N, cy * N, r * N
        notch = rng.uniform(0, 2 * math.pi)
        # Disque avec encoche (secteur vide de ~35°).
        pts = []
        for i in range(90):
            a = notch + 0.3 + i / 89 * (2 * math.pi - 0.6)
            rr = r * (1 + 0.03 * math.sin(a * 7))
            pts.append((cx + math.cos(a) * rr, cy + math.sin(a) * rr))
        pts.append((cx, cy))
        base = (0.16 + rng.uniform(-0.02, 0.02), 0.34, 0.12)
        d.polygon(pts, fill=rgb(base))
        # Bord plus clair et nervures rayonnantes.
        d.line(pts[:-1], fill=rgb((0.3, 0.46, 0.18)), width=int(N * 0.008))
        for k in range(11):
            a = notch + 0.3 + k / 10 * (2 * math.pi - 0.6)
            d.line([(cx, cy), (cx + math.cos(a) * r * 0.92, cy + math.sin(a) * r * 0.92)], fill=rgb((0.26, 0.42, 0.16)), width=int(N * 0.004))
    # Fleur blanc-rosé sur la plus grande feuille.
    fx, fy = 0.36 * N, 0.4 * N
    for layer, (count, length, col) in enumerate([(10, 0.1, (0.95, 0.78, 0.84)), (8, 0.07, (0.99, 0.9, 0.93))]):
        for k in range(count):
            a = k / count * 2 * math.pi + layer * 0.3
            px, py = fx + math.cos(a) * length * N * 0.55, fy + math.sin(a) * length * N * 0.55
            w = length * N * 0.28
            d.ellipse([px - w, py - w, px + w, py + w], fill=rgb(col))
    d.ellipse([fx - N * 0.018, fy - N * 0.018, fx + N * 0.018, fy + N * 0.018], fill=rgb((0.95, 0.8, 0.25)))
    return img


def kelp(rng):
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # Lame ondulée le long d'un stipe central, du pied (bas) à la pointe.
    for f in range(3):
        x0 = N * (0.35 + 0.15 * f)
        phase = rng.uniform(0, 6.28)
        left, right = [], []
        steps = 60
        for i in range(steps + 1):
            t = i / steps
            x = x0 + math.sin(t * 9 + phase) * N * 0.04
            y = N - t * N * 0.98
            w = N * (0.05 + 0.07 * math.sin(math.pi * min(1, t * 1.2)) ) * (1 + 0.25 * math.sin(t * 40 + phase))
            left.append((x - w, y))
            right.append((x + w, y))
        for i in range(steps):
            c = lerp((0.2, 0.18, 0.06), (0.42, 0.38, 0.12), i / steps)
            d.polygon([left[i], left[i + 1], right[i + 1], right[i]], fill=rgb(c))
        pts = [(x0 + math.sin(i / steps * 9 + phase) * N * 0.04, N - i / steps * N * 0.98) for i in range(steps + 1)]
        d.line(pts, fill=rgb((0.3, 0.25, 0.08)), width=int(N * 0.01))
    return img


def coral(rng):
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def branch(x, y, angle, length, width, depth):
        x2 = x + math.cos(angle) * length
        y2 = y - math.sin(angle) * length
        c = rgb((0.88, 0.86, 0.82))
        d.line([(x, y), (x2, y2)], fill=c, width=max(2, int(width)))
        d.ellipse([x2 - width / 2, y2 - width / 2, x2 + width / 2, y2 + width / 2], fill=c)
        if depth == 0:
            # Bout arrondi plus clair (polypes).
            r = width * 0.8
            d.ellipse([x2 - r, y2 - r, x2 + r, y2 + r], fill=rgb((0.97, 0.95, 0.9)))
            return
        for k in range(2 if rng.random() < 0.6 else 3):
            branch(x2, y2, angle + rng.uniform(-0.75, 0.75), length * rng.uniform(0.72, 0.9), width * 0.8, depth - 1)

    for _ in range(4):
        branch(N * rng.uniform(0.3, 0.7), N, math.pi / 2 + rng.uniform(-0.5, 0.5), N * 0.16, N * 0.032, 5)
    return img


def finish(img):
    small = img.resize((TILE, TILE), Image.Resampling.LANCZOS)
    a = np.array(small).astype(np.float32) / 255
    color, alpha = a[..., :3], a[..., 3:]
    prem = Image.fromarray((np.concatenate([color * alpha, alpha], -1) * 255).astype(np.uint8), "RGBA")
    fill = None
    for radius in (2, 6, 16, 48):
        blur = np.array(prem.filter(ImageFilter.GaussianBlur(radius))).astype(np.float32) / 255
        c = blur[..., :3] / np.maximum(blur[..., 3:], 1e-4)
        mask = blur[..., 3:] > 1e-3
        fill = np.where(mask, c, fill if fill is not None else c)
    out = np.concatenate([np.where(alpha > 0.02, color, fill), alpha], -1)
    return (np.clip(out, 0, 1) * 255).astype(np.uint8)


rng = random.Random(11)
tiles = [("reed.png", reed(rng)), ("lily_pad.png", lily(rng)), ("kelp.png", kelp(rng)), ("coral.png", coral(rng))]
for k, (name, img) in enumerate(tiles):
    x, y = col_x, MARGIN + (2 + k) * SLOT
    color = finish(img)
    alpha = color[..., 3:]
    normal = np.concatenate([np.full(color.shape[:2] + (1,), 128), np.full(color.shape[:2] + (1,), 128), np.full(color.shape[:2] + (1,), 255), alpha], -1).astype(np.uint8)
    rough = np.concatenate([np.full(color.shape[:2] + (1,), 255), np.full(color.shape[:2] + (1,), 170), np.zeros(color.shape[:2] + (1,)), alpha], -1).astype(np.uint8)
    for atlas, tile in zip(atlases, (color, normal, rough)):
        atlas.paste(Image.fromarray(tile, "RGBA"), (x, y))
    meta["frames"][name] = {"frame": {"x": x, "y": y, "w": TILE, "h": TILE}}

for im, f in zip(atlases, FILES):
    im.save(ASSETS + f)
json.dump(meta, open(JSON, "w"), indent=2)
print("tuiles ajoutées :", [n for n, _ in tiles])
