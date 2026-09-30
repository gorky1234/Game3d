"""Synthétise les boucles sonores de l'eau (assets/sounds/*.ogg) :
- river.ogg : murmure d'un cours d'eau (bruit filtré + glouglous) ;
- waterfall.ogg : grondement d'une cascade (bruit large bande).
Le bruit est mis en forme dans le domaine fréquentiel sur toute la durée de
la boucle (FFT circulaire) et les glouglous débordant de la fin reviennent
au début : la boucle n'a aucune couture. Nécessite numpy et ffmpeg
(libvorbis)."""
import os, subprocess, tempfile, wave
import numpy as np

RATE = 44100
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "sounds")


def shaped_noise(rng, seconds, gain_of_freq):
    n = int(RATE * seconds)
    spectrum = np.fft.rfft(rng.standard_normal(n))
    freqs = np.fft.rfftfreq(n, 1.0 / RATE)
    spectrum *= gain_of_freq(np.maximum(freqs, 1.0))
    x = np.fft.irfft(spectrum, n)
    return x / np.max(np.abs(x))


def band(f, lo, hi, order=2.0):
    """Passe-bande doux (pentes en puissance de `order`)."""
    return 1.0 / (1.0 + (lo / f) ** order) / (1.0 + (f / hi) ** order)


def slow_mod(rng, n, periods, depth):
    """Modulation lente d'amplitude, périodique sur la boucle."""
    t = np.arange(n) / n
    m = np.ones(n)
    for p in periods:
        m += depth * np.sin(2 * np.pi * (p * t + rng.random()))
    return m / m.max()


def gurgles(rng, n, count):
    """Glouglous : courtes sinusoïdes glissantes (bulles), placées au hasard,
    repliées sur la boucle."""
    out = np.zeros(n)
    for _ in range(count):
        dur = rng.uniform(0.02, 0.09)
        k = int(dur * RATE)
        t = np.arange(k) / RATE
        f0 = rng.uniform(350, 1400)
        f = f0 * (1 + rng.uniform(0.3, 1.2) * t / dur)  # la bulle monte
        phase = 2 * np.pi * np.cumsum(f) / RATE
        env = np.sin(np.pi * t / dur) ** 2 * np.exp(-t / (dur * 0.6))
        start = rng.integers(0, n)
        idx = (start + np.arange(k)) % n
        out[idx] += np.sin(phase) * env * rng.uniform(0.3, 1.0)
    return out / max(np.max(np.abs(out)), 1e-9)


def save_ogg(name, x):
    x = (x / np.max(np.abs(x)) * 0.9 * 32767).astype(np.int16)
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
        path = tmp.name
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(x.tobytes())
    out = os.path.join(OUT, name)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", path, "-c:a", "libvorbis", "-q:a", "5", out], check=True)
    os.remove(path)
    print("écrit", out)


def main():
    rng = np.random.default_rng(7)
    os.makedirs(OUT, exist_ok=True)

    seconds = 12.0
    n = int(RATE * seconds)
    # Murmure : bruit coloré (pente ~1/f) centré sur 200 Hz - 3 kHz, bosse
    # vers 600 Hz (clapotis), respiration lente, glouglous par-dessus.
    base = shaped_noise(rng, seconds, lambda f: band(f, 120, 3200) / np.sqrt(f / 300) * (1 + 1.5 * band(f, 450, 900, 3)))
    splash = shaped_noise(rng, seconds, lambda f: band(f, 1500, 7000))
    river = base * slow_mod(rng, n, [2, 3, 7], 0.25) + 0.12 * splash * slow_mod(rng, n, [5, 11, 17], 0.6)
    river += 0.35 * gurgles(rng, n, int(seconds * 14))
    save_ogg("river.ogg", river)

    # Cascade : grondement large bande (graves puissants, souffle aigu),
    # peu modulé.
    seconds = 10.0
    n = int(RATE * seconds)
    roar = shaped_noise(rng, seconds, lambda f: band(f, 60, 5000, 1.5) / (f / 150) ** 0.35)
    hiss = shaped_noise(rng, seconds, lambda f: band(f, 2500, 12000))
    waterfall = roar * slow_mod(rng, n, [3, 7], 0.12) + 0.25 * hiss * slow_mod(rng, n, [13, 19], 0.3)
    save_ogg("waterfall.ogg", waterfall)


if __name__ == "__main__":
    main()
