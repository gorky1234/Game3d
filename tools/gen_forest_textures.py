"""Agrandit l'atlas (couleur, normales, rugosité) de 5x5 à 6x6 emplacements et
y ajoute les textures du sous-bois et des nouvelles essences :
- fern.png : fronde de fougère (carte à découpe alpha, base en bas) ;
- birch.png : écorce de bouleau (blanche, lenticelles sombres, taches noires).
Les emplacements existants ne bougent pas (les UV sont recalculées depuis le
JSON au démarrage). Relancer sur un atlas déjà agrandi remplace simplement
les deux tuiles.

Usage : python3 tools/gen_forest_textures.py
"""
import json, math, random
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ASSETS = "assets/"
COLOR, NORMAL, ROUGH = (ASSETS + f for f in ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png"))
JSON = ASSETS + "atlas_texture.json"
TILE, MARGIN = 1024, 32
SLOT = TILE + 2 * MARGIN
SIZE = 6 * SLOT

rng = random.Random(11)

def grow(path, fill):
    im = Image.open(path).convert("RGBA")
    if im.size[0] >= SIZE:
        return im
    out = Image.new("RGBA", (SIZE, SIZE), fill)
    out.paste(im, (0, 0))
    return out

def put(atlas, tile, x, y, wrap):
    """Pose `tile` en (x, y) ; marge = contenu enroulé (tuile répétable) ou
    pixels du bord étirés."""
    a = np.array(tile)
    if wrap:
        padded = np.pad(a, ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="wrap")
    else:
        padded = np.pad(a, ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="edge")
    atlas.paste(Image.fromarray(padded, "RGBA"), (x - MARGIN, y - MARGIN))

# --- Fronde de fougère ---

def fern():
    ss = 4
    n = TILE * ss
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # Rachis : de la base (en bas, au centre) à la pointe, légèrement courbé.
    steps = 60
    pts = []
    for i in range(steps + 1):
        t = i / steps
        pts.append((n * (0.5 + 0.04 * math.sin(t * 2.5)), n * (0.98 - 0.94 * t)))
    for i in range(steps):
        t = i / steps
        d.line([pts[i], pts[i + 1]], fill=(70, 95, 40, 255), width=int(ss * (5 - 3 * t)))
    # Pennes : paires opposées, longues au milieu, courtes à la base et à la
    # pointe ; chacune découpée en lobes.
    count = 15
    for k in range(count):
        t = 0.08 + 0.9 * k / count
        i = int(t * steps)
        bx, by = pts[i]
        env = math.sin(min(1.0, t * 1.15) * math.pi) ** 0.7
        length = n * 0.45 * env * (1 - 0.4 * t) + n * 0.03
        for side in (-1, 1):
            ang = math.radians(90 - 12 - 25 * t) * side
            dx, dy = math.sin(ang), -math.cos(ang)
            base = np.array([0.40, 0.55, 0.22]) * (0.8 + 0.25 * rng.random())
            tip = np.array([0.55, 0.70, 0.30]) * (0.85 + 0.2 * rng.random())
            lobes = max(4, int(length / (ss * 13)))
            for j in range(lobes):
                u = j / lobes
                cx = bx + dx * length * u
                cy = by + dy * length * u - length * 0.08 * u * u
                w = ss * 13 * (1 - 0.6 * u) * (0.7 + 0.3 * env)
                col = base + (tip - base) * u
                c = tuple(int(255 * v) for v in col) + (255,)
                # Lobe : ellipse allongée perpendiculairement à la penne.
                d.ellipse([cx - w * 0.8, cy - w * 1.1, cx + w * 0.8, cy + w * 1.1], fill=c)
            d.line([(bx, by), (bx + dx * length, by + dy * length - length * 0.08)], fill=(60, 85, 35, 255), width=ss * 2)
    small = img.resize((TILE, TILE), Image.Resampling.LANCZOS)
    # Couleur étendue sous le transparent (pas de liseré dans les mipmaps).
    a = np.array(small).astype(np.float32) / 255
    rgb, alpha = a[..., :3], a[..., 3:]
    prem = Image.fromarray((np.concatenate([rgb * alpha, alpha], -1) * 255).astype(np.uint8), "RGBA")
    blur = np.array(prem.filter(ImageFilter.GaussianBlur(12))).astype(np.float32) / 255
    fill = blur[..., :3] / np.maximum(blur[..., 3:], 1e-4)
    fill = np.where(blur[..., 3:] > 1e-3, fill, np.array([0.4, 0.55, 0.22]))
    out = np.concatenate([np.where(alpha > 0.02, rgb, fill), alpha], -1)
    return Image.fromarray((np.clip(out, 0, 1) * 255).astype(np.uint8), "RGBA")

# --- Écorce de bouleau ---

def tile_noise(size, cell, seed):
    r = np.random.RandomState(seed)
    g = r.rand(size // cell + 1, size // cell + 1)
    g[-1, :] = g[0, :]
    g[:, -1] = g[:, 0]
    im = Image.fromarray((g * 255).astype(np.uint8)).resize((size + cell, size + cell), Image.Resampling.BICUBIC)
    return np.array(im)[:size, :size].astype(np.float32) / 255

def birch():
    s = TILE
    base = np.ones((s, s, 3)) * np.array([0.86, 0.84, 0.78])
    n = tile_noise(s, 64, 1) * 0.6 + tile_noise(s, 16, 2) * 0.4
    base *= (0.88 + 0.18 * n)[..., None]
    height = np.zeros((s, s))
    img = Image.fromarray((base * 255).astype(np.uint8), "RGB")
    d = ImageDraw.Draw(img)
    hmap = Image.new("L", (s, s), 128)
    hd = ImageDraw.Draw(hmap)
    r = random.Random(5)
    # Lenticelles : tirets horizontaux sombres (u = tour du tronc = x).
    for _ in range(170):
        x, y = r.uniform(0, s), r.uniform(0, s)
        w, h = r.uniform(10, 55), r.uniform(1.5, 4)
        c = int(r.uniform(35, 90))
        for ox in (-s, 0, s):
            for oy in (-s, 0, s):
                box = [x - w / 2 + ox, y - h / 2 + oy, x + w / 2 + ox, y + h / 2 + oy]
                d.ellipse(box, fill=(c, c - 3, c - 8))
                hd.ellipse(box, fill=60)
    # Grandes taches noires (écorce craquelée).
    for _ in range(5):
        x, y = r.uniform(0, s), r.uniform(0, s)
        w, h = r.uniform(30, 80), r.uniform(12, 30)
        for ox in (-s, 0, s):
            for oy in (-s, 0, s):
                pts = []
                for k in range(14):
                    a = k / 14 * math.tau
                    rr = 0.6 + 0.4 * r.random()
                    pts.append((x + ox + math.cos(a) * w / 2 * rr, y + oy + math.sin(a) * h / 2 * rr))
                d.polygon(pts, fill=(28, 26, 24))
                hd.polygon(pts, fill=30)
    color = Image.merge("RGBA", (*img.split(), Image.new("L", (s, s), 255)))
    # Normales depuis la carte de hauteur (creux sombres = entailles).
    h = np.array(hmap.filter(ImageFilter.GaussianBlur(1.5))).astype(np.float32) / 255
    gx = (np.roll(h, -1, 1) - np.roll(h, 1, 1)) * 2.5
    gy = (np.roll(h, -1, 0) - np.roll(h, 1, 0)) * 2.5
    nrm = np.stack([-gx, gy, np.ones_like(h)], -1)
    nrm /= np.linalg.norm(nrm, axis=-1, keepdims=True)
    normal = Image.fromarray(np.concatenate([((nrm * 0.5 + 0.5) * 255).astype(np.uint8), np.full((s, s, 1), 255, np.uint8)], -1), "RGBA")
    # Rugosité (G) : écorce lisse et un peu satinée, entailles mates.
    rough = (0.55 + 0.4 * (1 - h)) * 255
    mr = np.stack([np.full((s, s), 255), rough, np.zeros((s, s)), np.full((s, s), 255)], -1).astype(np.uint8)
    return color, normal, Image.fromarray(mr, "RGBA")

color = grow(COLOR, (0, 0, 0, 0))
normal = grow(NORMAL, (128, 128, 255, 255))
rough = grow(ROUGH, (255, 255, 0, 255))
meta = json.load(open(JSON))
slots = {"fern.png": (4 * SLOT + MARGIN, 4 * SLOT + MARGIN), "birch.png": (5 * SLOT + MARGIN, MARGIN)}

fx, fy = slots["fern.png"]
put(color, fern(), fx, fy, wrap=False)
put(normal, Image.new("RGBA", (TILE, TILE), (128, 128, 255, 255)), fx, fy, wrap=False)
put(rough, Image.new("RGBA", (TILE, TILE), (255, 235, 0, 255)), fx, fy, wrap=False)

bx, by = slots["birch.png"]
bc, bn, br = birch()
put(color, bc, bx, by, wrap=True)
put(normal, bn, bx, by, wrap=True)
put(rough, br, bx, by, wrap=True)

for name, (x, y) in slots.items():
    meta["frames"][name] = {"frame": {"x": x, "y": y, "w": TILE, "h": TILE}}
meta["meta"]["size"] = {"w": SIZE, "h": SIZE}
color.save(COLOR)
normal.save(NORMAL)
rough.save(ROUGH)
json.dump(meta, open(JSON, "w"), indent=2)
