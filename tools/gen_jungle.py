"""Textures de la jungle, dans une nouvelle colonne de l'atlas (couleur,
normales, rugosité), à partir des photos Poly Haven de tools/polyhaven :
- jungle_leaf.png : amas de grandes feuilles vernies (touffe des houppiers
  tropicaux, comme leaf_card.png pour les feuillus) ;
- palm_frond.png : palme pennée (rachis de bas en haut, folioles) ;
- broadleaf.png : grande feuille de bananier déchirée (sous-bois) ;
- liana.png : liane feuillue, raccordable verticalement ;
- litter.png : litière de feuilles mortes sur sol forestier (couche du
  terrain, raccordable) ;
- heliconia.png : hampe florale rouge et jaune (sous-bois).

Sources : forrest_ground_01 (sol), island_tree_02_leaves (feuilles).
L'atlas est agrandi à droite (largeur arrondie au multiple de 512 suivant,
voir pad_atlas.py) ; les UV des tuiles existantes sont recalculées par le
jeu depuis atlas_texture.json.

Usage : python3 tools/gen_jungle.py
"""
import json, math, random
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ASSETS = "assets/"
SRC = "tools/polyhaven/"
FILES = ("atlas_texture.png", "atlas_texture_normal.png", "atlas_texture_metallic_roughness.png")
JSON = ASSETS + "atlas_texture.json"
TILE, MARGIN = 1024, 32
SLOT = TILE + 2 * MARGIN
PAD = 512

meta = json.load(open(JSON))
atlases = [Image.open(ASSETS + f).convert("RGBA") for f in FILES]
names = ["jungle_leaf.png", "palm_frond.png", "broadleaf.png", "liana.png", "litter.png", "heliconia.png"]
if any(n in meta["frames"] for n in names):
    raise SystemExit("tuiles de la jungle déjà présentes dans l'atlas")

# Nouvelle colonne à droite de la dernière.
col_x = max(f["frame"]["x"] for f in meta["frames"].values()) + SLOT
width = -(-(col_x + TILE + MARGIN) // PAD) * PAD
height = atlases[0].height
grown = []
for im, fill in zip(atlases, ((0, 0, 0, 0), (128, 128, 255, 0), (255, 200, 0, 0))):
    big = Image.new("RGBA", (width, height), fill)
    big.paste(im, (0, 0))
    grown.append(big)
atlases = grown
meta["meta"]["size"]["w"] = width

# --- Feuilles photographiées, découpées une à une ---------------------------
leaf_rgb = Image.open(SRC + "island_tree_02_leaves_diff_2k.png").convert("RGB")
leaf_a = Image.open(SRC + "island_tree_02_leaves_alpha_2k.png").convert("L")
sheet = leaf_rgb.copy()
sheet.putalpha(leaf_a)
mask = np.array(leaf_a) > 128
# Composantes connexes grossières (colonnes de feuilles bien séparées).
small = Image.fromarray(mask.astype(np.uint8) * 255).resize((256, 256), Image.Resampling.BOX)
sm = np.array(small) > 40
seen = np.zeros_like(sm)
leaves = []
for y0 in range(256):
    for x0 in range(256):
        if sm[y0, x0] and not seen[y0, x0]:
            stack, pts = [(y0, x0)], []
            seen[y0, x0] = True
            while stack:
                y, x = stack.pop()
                pts.append((y, x))
                for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    ny, nx = y + dy, x + dx
                    if 0 <= ny < 256 and 0 <= nx < 256 and sm[ny, nx] and not seen[ny, nx]:
                        seen[ny, nx] = True
                        stack.append((ny, nx))
            ys, xs = zip(*pts)
            if max(ys) - min(ys) > 40:
                box = (min(xs) * 8, min(ys) * 8, (max(xs) + 1) * 8, (max(ys) + 1) * 8)
                crop = np.array(sheet.crop(box))
                # Éclats clairs de la planche photo (étiquettes, reflets) : retirés.
                light = crop[..., :3].astype(np.float32).mean(-1) > 170
                crop[light, 3] = 0
                leaves.append(Image.fromarray(crop, "RGBA"))
print("feuilles découpées :", len(leaves))


def tint(img, mul, add=(0, 0, 0)):
    a = np.array(img).astype(np.float32)
    a[..., :3] = np.clip(a[..., :3] * np.array(mul) + np.array(add), 0, 255)
    return Image.fromarray(a.astype(np.uint8), "RGBA")


def place(canvas, leaf, center, length, angle_deg, width_scale=1.0):
    """Feuille (base en bas) de `length` px, tournée de `angle_deg` (0 : vers
    le haut), sa base (pétiole) posée en `center`."""
    w, h = leaf.size
    scale = length / h
    lw = max(2, int(w * scale * width_scale))
    lf = leaf.resize((lw, max(2, int(length))), Image.Resampling.LANCZOS)
    pad = Image.new("RGBA", (lf.width, lf.height * 2), (0, 0, 0, 0))
    pad.paste(lf, (0, 0))
    rot = pad.rotate(-angle_deg, resample=Image.Resampling.BICUBIC, expand=True)
    canvas.alpha_composite(rot, (int(center[0] - rot.width / 2), int(center[1] - rot.height / 2)))


def finish(img):
    """Couleur étendue sous les zones transparentes (pas de liseré dans les
    mipmaps), comme les autres scripts de l'atlas."""
    small = img.resize((TILE, TILE), Image.Resampling.LANCZOS) if img.size != (TILE, TILE) else img
    a = np.array(small).astype(np.float32) / 255
    color, alpha = a[..., :3], a[..., 3:]
    prem = Image.fromarray((np.concatenate([color * alpha, alpha], -1) * 255).astype(np.uint8), "RGBA")
    fill = None
    for radius in (2, 6, 16, 48):
        blur = np.array(prem.filter(ImageFilter.GaussianBlur(radius))).astype(np.float32) / 255
        c = blur[..., :3] / np.maximum(blur[..., 3:], 1e-4)
        m = blur[..., 3:] > 1e-3
        fill = np.where(m, c, fill if fill is not None else c)
    out = np.concatenate([np.where(alpha > 0.02, color, fill), alpha], -1)
    return (np.clip(out, 0, 1) * 255).astype(np.uint8)


rng = random.Random(23)
N = 2048  # dessin à 2x, réduit à la fin


def jungle_leaf():
    """Touffe de grandes feuilles vernies qui se chevauchent au hasard
    (orientations et tailles variées, pas une étoile régulière), vert sombre,
    quelques jeunes feuilles plus claires ; bord de la touffe irrégulier."""
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    for _ in range(18):
        leaf = rng.choice(leaves)
        young = rng.random() < 0.2
        leaf = tint(leaf, (0.55, 0.8, 0.5) if not young else (0.8, 1.02, 0.6))
        # Pétiole près du centre, feuille orientée plutôt vers l'extérieur.
        r = N * rng.uniform(0.0, 0.18)
        a = rng.uniform(0, 360)
        rad = math.radians(a)
        base = (N / 2 + math.sin(rad) * r, N / 2 - math.cos(rad) * r)
        out = a + rng.uniform(-60, 60)
        place(img, leaf, base, N * rng.uniform(0.2, 0.36), out, rng.uniform(1.1, 1.6))
    # Cœur de la touffe : feuilles couchées en travers du centre (sinon un
    # trou au milieu).
    for _ in range(5):
        leaf = tint(rng.choice(leaves), (0.62, 0.88, 0.55))
        a = rng.uniform(0, 360)
        L = N * rng.uniform(0.2, 0.28)
        rad = math.radians(a)
        base = (N / 2 - math.sin(rad) * L * 0.45, N / 2 + math.cos(rad) * L * 0.45)
        place(img, leaf, base, L, a, 1.4)
    return img


def palm_frond():
    """Palme : rachis courbe de bas en haut, folioles fines de part et
    d'autre, plus courtes vers la pointe."""
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pts = [(N * 0.5 + math.sin(t * 1.2) * N * 0.04, N * (0.98 - 0.94 * t)) for t in np.linspace(0, 1, 40)]
    d.line(pts, fill=(95, 105, 45, 255), width=int(N * 0.012))
    for i, t in enumerate(np.linspace(0.08, 0.97, 34)):
        x, y = pts[int(t * 39)]
        for side in (-1, 1):
            leaf = tint(rng.choice(leaves), (0.6, 0.82, 0.5))
            L = N * (0.34 * (1 - t) ** 0.6 + 0.05) * rng.uniform(0.9, 1.1)
            ang = side * rng.uniform(55, 75)
            place(img, leaf, (x, y), L, ang, 0.28)
    return img


def broadleaf():
    """Grande feuille de bananier : la plus large des photos, étirée sur la
    tuile, pétiole en bas, déchirures perpendiculaires à la nervure."""
    leaf = max(leaves, key=lambda l: l.width / l.height)
    leaf = tint(leaf, (0.66, 0.92, 0.55))
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    place(img, leaf, (N / 2, N * 0.9), N * 0.86, 0, 1.6)
    d = ImageDraw.Draw(img)
    d.line([(N / 2, N * 0.87), (N / 2, N)], fill=(110, 125, 55, 255), width=int(N * 0.018))
    a = np.array(img)
    for _ in range(9):
        y = int(N * rng.uniform(0.12, 0.8))
        side = rng.choice((-1, 1))
        # Fente depuis le bord jusqu'à la nervure, légèrement oblique.
        for x in range(N // 2 + side * int(N * 0.02), N // 2 + side * N // 2, side):
            yy = int(y - (x - N / 2) * side * 0.35)
            if 0 <= yy < N:
                a[max(0, yy - 3):yy + 3, x, 3] = 0
    return Image.fromarray(a, "RGBA")


def liana():
    """Liane : tige ligneuse épaisse et torsadée de haut en bas (raccordable
    verticalement), feuillage dense de part et d'autre."""
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    ts = np.linspace(0, 1, 80)
    xs = [N * 0.5 + math.sin(t * math.tau * 2) * N * 0.06 for t in ts]
    ys = [t * N for t in ts]
    # Deux brins entrelacés, clair et sombre (relief de la tige).
    d.line(list(zip(xs, ys)), fill=(70, 58, 38, 255), width=int(N * 0.05))
    xs2 = [N * 0.5 + math.sin(t * math.tau * 2 + 1.6) * N * 0.045 for t in ts]
    d.line(list(zip(xs2, ys)), fill=(105, 90, 60, 255), width=int(N * 0.028))
    for i, t in enumerate(np.linspace(0.0, 1.0, 26)):
        k = min(int(t * 79), 79)
        x, y = xs[k], ys[k]
        side = 1 if i % 2 else -1
        leaf = tint(rng.choice(leaves), (0.6, 0.88, 0.52) if rng.random() < 0.7 else (0.75, 1.0, 0.6))
        L = N * rng.uniform(0.18, 0.28)
        ang = side * rng.uniform(70, 150)
        place(img, leaf, (x, y), L, ang, 1.3)
    return img


def heliconia():
    """Hampe d'héliconia : bractées en bateau (dessous arrondi, arête droite
    qui remonte vers la pointe), rouge vif dégradé vers la pointe, arête
    jaune, alternées sur une tige en zigzag, plus petites vers le haut."""
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    count = 8
    pts = [(N / 2 + (1 if i % 2 else -1) * N * 0.025, N * (0.97 - i * 0.105)) for i in range(count + 1)]
    d.line(pts, fill=(95, 105, 40, 255), width=int(N * 0.022))
    for i in range(count):
        x, y = pts[i]
        side = 1 if i % 2 else -1
        L = N * (0.3 - i * 0.022)
        H = N * (0.085 - i * 0.005)
        tip = (x + side * L, y - L * 0.35)
        # Dessous : courbe du pied à la pointe, creusée de H vers le bas.
        steps = 24
        bottom = []
        for k in range(steps + 1):
            u = k / steps
            bx = x + side * L * u
            by = y + (tip[1] - y) * u + H * math.sin(math.pi * u) ** 0.8
            bottom.append((bx, by))
        poly = bottom + [(x, y)]
        # Dégradé : bandes du pied (rouge sombre) à la pointe (rouge-orangé).
        for k in range(steps):
            u = k / steps
            seg = [(x + side * L * u, y + (tip[1] - y) * u), bottom[k], bottom[k + 1], (x + side * L * (u + 1 / steps), y + (tip[1] - y) * (u + 1 / steps))]
            d.polygon(seg, fill=(int(190 + 40 * u), int(20 + 35 * u), 25, 255))
        d.line([(x, y), tip], fill=(230, 195, 50, 255), width=int(N * 0.011))
        _ = poly
    return img.filter(ImageFilter.GaussianBlur(1.0))


def litter():
    """Sol de jungle : terre forestière brun sombre couverte de petites
    feuilles mortes (photos recolorées brun, roux, ocre, quelques vertes),
    raccordable. Feuilles petites et peu contrastées : pas de motif."""
    base = Image.open(SRC + "forrest_ground_01_diff_2k.png").convert("RGBA").resize((N, N), Image.Resampling.LANCZOS)
    base = tint(base, (0.6, 0.48, 0.38))
    # Teintes proches les unes des autres (brun, roux, ocre) : les feuilles
    # vertes et très sombres faisaient un sol « camouflage ».
    colors = [(0.66, 0.4, 0.24), (0.74, 0.48, 0.27), (0.6, 0.38, 0.23), (0.78, 0.54, 0.3), (0.62, 0.44, 0.26)]
    for _ in range(1400):
        # Feuilles photographiées éclaircies (vert sombre à l'origine) et un
        # peu transparentes : elles se fondent dans le sol.
        leaf = tint(rng.choice(leaves), rng.choice(colors), (26, 16, 6))
        a = np.array(leaf)
        a[..., 3] = (a[..., 3] * 0.85).astype(np.uint8)
        leaf = Image.fromarray(a, "RGBA")
        L = N * rng.uniform(0.025, 0.055)
        c = (rng.uniform(0, N), rng.uniform(0, N))
        ang = rng.uniform(0, 360)
        # Raccordable : la feuille est aussi posée de l'autre côté du bord.
        for ox in (-N, 0, N):
            for oy in (-N, 0, N):
                cx, cy = c[0] + ox, c[1] + oy
                if -L < cx < N + L and -L < cy < N + L:
                    place(base, leaf, (cx, cy), L, ang, 1.3)
    base.putalpha(255)
    return base


def flat_normal(alpha):
    return np.concatenate([np.full(alpha.shape[:2] + (1,), 128), np.full(alpha.shape[:2] + (1,), 128), np.full(alpha.shape[:2] + (1,), 255), alpha], -1).astype(np.uint8)


def rough_map(alpha, rough):
    return np.concatenate([np.full(alpha.shape[:2] + (1,), 255), np.full(alpha.shape[:2] + (1,), rough), np.zeros(alpha.shape[:2] + (1,)), alpha], -1).astype(np.uint8)


tiles = [("jungle_leaf.png", jungle_leaf(), 120), ("palm_frond.png", palm_frond(), 130), ("broadleaf.png", broadleaf(), 120),
         ("liana.png", liana(), 150), ("litter.png", litter(), None), ("heliconia.png", heliconia(), 110)]
for k, (name, img, rough) in enumerate(tiles):
    x, y = col_x, MARGIN + k * SLOT
    color = finish(img)
    alpha = color[..., 3:]
    if name == "litter.png":
        nor = Image.open(SRC + "forrest_ground_01_nor_gl_2k.png").convert("RGBA").resize((TILE, TILE), Image.Resampling.LANCZOS)
        rgh = Image.open(SRC + "forrest_ground_01_rough_2k.png").convert("L").resize((TILE, TILE), Image.Resampling.LANCZOS)
        normal = np.array(nor)
        normal[..., 3] = 255
        rough_tile = rough_map(alpha, 0)
        rough_tile[..., 1] = np.array(rgh)
    else:
        normal = flat_normal(alpha)
        rough_tile = rough_map(alpha, rough)
    for atlas, tile in zip(atlases, (color, normal, rough_tile)):
        atlas.paste(Image.fromarray(tile, "RGBA"), (x, y))
    meta["frames"][name] = {"frame": {"x": x, "y": y, "w": TILE, "h": TILE}}

for im, f in zip(atlases, FILES):
    im.save(ASSETS + f)
json.dump(meta, open(JSON, "w"), indent=2)
print("colonne ajoutée en x =", col_x, "; atlas", width, "x", height, "; tuiles :", [n for n, _, _ in tiles])
