"""Tuiles de la flore au sol propre aux biomes (voir `plant_mesh`,
src/render/plant_mesh.rs), jusqu'ici empruntées à d'autres plantes et
reteintées :

- moss.png : coussin de mousse (tourbières, taïga, sous-bois) ;
- lichen.png : touffes de lichen des rennes (toundra), gris-vert ramifié ;
- flower_blue.png : campanules et gentianes (alpages, toundra) ;
- flower_purple.png : épis de petites fleurs violettes.

Fleurs vues de côté (cartes en croix, le pied en bas de la tuile) ; mousse
et lichen vus de dessus (plaques posées à plat sur le sol). Dans la 7e
rangée de l'atlas, à côté de salt.png (voir tools/gen_terrain_layers.py, à
lancer avant). Seul l'atlas de couleur compte (les plantes n'ont ni carte
de normales ni de rugosité).

Usage : python3 tools/gen_ground_flora.py
"""
import json, math, random, sys
import numpy as np
from PIL import Image, ImageDraw

sys.argv = sys.argv[:1]
# Fonctions de découpe et de pose des photos (voir gen_species.py).
src = open("tools/gen_photo_foliage.py").read()
exec(src[:src.index("atlas = Image.open(ATLAS)")])
rng = random.Random(61)

MARGIN = 32
SLOT = TILE + 2 * MARGIN
atlas = Image.open(ATLAS).convert("RGBA")
meta = json.load(open(JSON))
ROW = 6
assert "salt.png" in meta["frames"], "lancer tools/gen_terrain_layers.py d'abord"


def slot(col, row):
    return (col * SLOT + MARGIN, row * SLOT + MARGIN)


def blob_mask(cx, cy, radius, lobes, seed):
    """Tache irrégulière (vue de dessus) : rayon perturbé par des lobes."""
    r = random.Random(seed)
    phases = [(r.uniform(0, math.tau), r.uniform(0.03, 0.07), k) for k in range(2, 2 + lobes)]
    yy, xx = np.mgrid[0:N, 0:N].astype(np.float32)
    dx, dy = xx - cx, yy - cy
    angle = np.arctan2(dy, dx)
    rad = radius * (1 + sum(a * np.sin(k * angle + p) for p, a, k in phases))
    return np.hypot(dx, dy) / rad


# --- Mousse (vue de dessus) : coussin dense de petites tiges étoilées ---

def moss_tile():
    dist = blob_mask(N / 2, N / 2, N * 0.34, 6, 71)
    # Fond : vert moyen, à peine plus sombre sur le pourtour (pas de liseré).
    shade = np.clip(1.08 - 0.2 * dist, 0.8, 1.1)
    base = np.dstack([70 * shade, 94 * shade, 38 * shade, np.where(dist < 0.93, 255, 0)])
    img = Image.fromarray(np.clip(base, 0, 255).astype(np.uint8), "RGBA")
    d = ImageDraw.Draw(img)
    # Pointes des tiges : petites étoiles, plus jaunes au sommet ; celles du
    # bord dépassent (contour frangé au lieu d'un trait net).
    for _ in range(12000):
        x, y = rng.uniform(0, N), rng.uniform(0, N)
        k = float(dist[int(y), int(x)])
        if k >= 1.05:
            continue
        light = (1.08 - 0.25 * k) * rng.uniform(0.75, 1.2)
        yellow = rng.uniform(0.0, 0.25)
        col = (int(min(255, (100 + 60 * yellow) * light)), int(min(255, 128 * light)), int(min(255, 50 * light)), 255)
        size = rng.uniform(5, 11)
        for a in range(3):
            t = a * math.pi / 3 + rng.uniform(0, 1)
            d.line([(x - math.cos(t) * size, y - math.sin(t) * size), (x + math.cos(t) * size, y + math.sin(t) * size)], fill=col, width=3)
    return img


# --- Lichen des rennes (vue de dessus) : touffes pâles très ramifiées ---

def lichen_tile():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    clumps = [(N * rng.uniform(0.25, 0.75), N * rng.uniform(0.25, 0.75), N * rng.uniform(0.12, 0.2)) for _ in range(7)]
    for cx, cy, radius in clumps:
        # Creux sombres entre les rameaux, puis les pointes, claires.
        d.ellipse([cx - radius, cy - radius, cx + radius, cy + radius], fill=(118, 124, 104, 255))
        for _ in range(1400):
            a, rr = rng.uniform(0, math.tau), radius * math.sqrt(rng.uniform(0, 1))
            x, y = cx + math.cos(a) * rr, cy + math.sin(a) * rr
            v = rng.uniform(0.85, 1.12) * (1.05 - 0.25 * rr / radius)
            r = rng.uniform(4, 9)
            d.ellipse([x - r, y - r, x + r, y + r], fill=(int(min(255, 196 * v)), int(min(255, 202 * v)), int(min(255, 176 * v)), 255))
    return img


# --- Campanules et gentianes ---

def bell(d, x, y, size, angle, color):
    """Clochette pendante (campanule) : corolle ouverte vers le bas."""
    ca, sa = math.cos(angle), math.sin(angle)
    def rot(px, py):
        return (x + px * ca - py * sa, y + px * sa + py * ca)
    pts = [rot(size * 0.35 * math.sin(t), size * (0.1 + 0.9 * t)) for t in np.linspace(0, math.pi / 2, 8)]
    pts = [rot(0, 0)] + pts + [rot(size * 0.55, size * 1.05), rot(-size * 0.55, size * 1.05)] + \
          [rot(-size * 0.35 * math.sin(t), size * (0.1 + 0.9 * t)) for t in np.linspace(math.pi / 2, 0, 8)]
    d.polygon(pts, fill=color)
    dark = tuple(int(c * 0.6) for c in color[:3]) + (255,)
    d.line([rot(-size * 0.5, size * 1.02), rot(size * 0.5, size * 1.02)], fill=dark, width=5)


def blue_tile():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    for _ in range(16):
        stamp(img, rng.choice(blades), (N * rng.uniform(0.15, 0.85), N - 2), rng.uniform(-0.35, 0.35),
              N * rng.uniform(0.2, 0.38), 0.8, stretch=0.5)
    d = ImageDraw.Draw(img)
    for _ in range(6):
        x = N * rng.uniform(0.15, 0.85)
        top = (x + rng.uniform(-60, 60), N * rng.uniform(0.15, 0.4))
        d.line([(x, N - 2), top], fill=(78, 100, 58, 255), width=7)
        # Grappe de clochettes le long du haut de la tige.
        for j in range(rng.randint(4, 6)):
            t = j / 6
            px = top[0] + (x - top[0]) * t * 0.4
            py = top[1] + (N - top[1]) * t * 0.35
            v = rng.uniform(0.85, 1.1)
            color = (int(70 * v), int(92 * v), int(210 * v), 255)
            side = 1 if j % 2 else -1
            d.line([(px, py), (px + side * 40, py + 14)], fill=(78, 100, 58, 255), width=5)
            bell(d, px + side * 40, py + 14, rng.uniform(70, 95), side * rng.uniform(0.2, 0.5), color)
    return img


# --- Épis violets ---

def purple_tile():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    for _ in range(14):
        stamp(img, rng.choice(blades), (N * rng.uniform(0.15, 0.85), N - 2), rng.uniform(-0.35, 0.35),
              N * rng.uniform(0.2, 0.35), 0.8, stretch=0.5)
    d = ImageDraw.Draw(img)
    for _ in range(7):
        x = N * rng.uniform(0.12, 0.88)
        top = (x + rng.uniform(-50, 50), N * rng.uniform(0.12, 0.35))
        d.line([(x, N - 2), top], fill=(80, 98, 58, 255), width=7)
        # Épi : fleurettes serrées, plus petites et plus foncées vers la
        # pointe (boutons pas encore ouverts).
        spike = rng.uniform(260, 380)
        for j in range(60):
            t = j / 60
            fx = top[0] + (x - top[0]) * t * spike / (N - top[1]) + rng.uniform(-22, 22) * (1 - 0.6 * (1 - t))
            fy = top[1] + spike * t
            r = 7 + 9 * t
            v = rng.uniform(0.85, 1.1) * (0.7 + 0.3 * t)
            d.ellipse([fx - r, fy - r, fx + r, fy + r], fill=(int(150 * v), int(70 * v), int(185 * v), 255))
    return img


tiles = {
    "moss.png": moss_tile(),
    "lichen.png": lichen_tile(),
    "flower_blue.png": blue_tile(),
    "flower_purple.png": purple_tile(),
}
for col, (name, img) in enumerate(tiles.items(), start=1):
    pos = slot(col, ROW)
    tile = finish(img)
    padded = np.pad(np.array(tile), ((MARGIN, MARGIN), (MARGIN, MARGIN), (0, 0)), mode="edge")
    atlas.paste(Image.fromarray(padded, "RGBA"), (pos[0] - MARGIN, pos[1] - MARGIN))
    meta["frames"][name] = {"frame": {"x": pos[0], "y": pos[1], "w": TILE, "h": TILE}}
    print(name, "couverture", round(float((np.array(tile)[..., 3] > 128).mean()), 3))
atlas.save(ATLAS)
json.dump(meta, open(JSON, "w"), indent=2)
