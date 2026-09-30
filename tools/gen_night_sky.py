"""Ciel nocturne réel à partir des cartes de la NASA (domaine public) :

- assets/starmap.jpg : Deep Star Maps 2020 (NASA/Goddard Scientific
  Visualization Studio, https://svs.gsfc.nasa.gov/4851), voie lactée et
  étoiles en coordonnées célestes équirectangulaires, 8192 x 4096. Valeurs
  linéaires multipliées par GAIN puis encodées en sRGB (8 bits) : le fond de
  la voie lactée, très faible, garde assez de niveaux ; les étoiles
  brillantes saturent (des points).
- assets/moon.jpg : CGI Moon Kit (NASA SVS, https://svs.gsfc.nasa.gov/4720),
  couleur LRO, équirectangulaire 2048 x 1024.

Usage : python3 tools/gen_night_sky.py (sources dans tools/nasa/, voir le
docstring pour les URL : starmap_2020_8k.exr, lroc_color_2k.jpg).
"""
import OpenEXR
import numpy as np
from PIL import Image, ImageFilter

GAIN = 6.0

rgb = OpenEXR.File("tools/nasa/starmap_2020_8k.exr").channels()["RGB"].pixels.astype(np.float32)
lin = np.clip(rgb * GAIN, 0.0, 1.0)
srgb = np.where(lin <= 0.0031308, lin * 12.92, 1.055 * np.power(lin, 1 / 2.4) - 0.055)
# Netteté : léger masque flou (étoiles plus ponctuelles, poussières plus
# nettes), JPEG sans sous-échantillonnage des couleurs (4:4:4 ; en 4:2:0, les
# étoiles colorées bavaient sur 2 pixels).
img = Image.fromarray((srgb * 255 + 0.5).astype(np.uint8), "RGB").filter(ImageFilter.UnsharpMask(radius=1.2, percent=70, threshold=2))
img.save("assets/starmap.jpg", quality=95, subsampling=0)
print("starmap", rgb.shape, "p50 codé", int(np.percentile(srgb, 50) * 255), "p99", int(np.percentile(srgb, 99) * 255))

Image.open("tools/nasa/lroc_color_2k.jpg").convert("RGB").save("assets/moon.jpg", quality=92)
