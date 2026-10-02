"""Synthétise le tonnerre (assets/sounds/thunder.ogg, voir les éclairs dans
src/render/skybox.rs) : un claquement sec, puis un grondement grave qui roule
et s'éteint en ~6 s (bruit filtré passe-bas, enveloppe en rafales).

Usage : python3 tools/gen_thunder.py (nécessite numpy et soundfile).
"""
import numpy as np
import soundfile as sf

RATE = 44100
rng = np.random.default_rng(17)
n = int(RATE * 7.0)
t = np.arange(n) / RATE


def lowpass(x, cutoff):
    # Filtre passe-bas du 1er ordre, appliqué deux fois (pente douce).
    a = np.exp(-2 * np.pi * cutoff / RATE)
    for _ in range(2):
        y = np.empty_like(x)
        acc = 0.0
        for i in range(len(x)):
            acc = (1 - a) * x[i] + a * acc
            y[i] = acc
        x = y
    return x


noise = rng.standard_normal(n)
crack = lowpass(noise, 2500.0) * np.exp(-t / 0.08) * (t < 0.6)
rumble = lowpass(noise, 140.0) * 6.0
# Rafales : le grondement roule en vagues irrégulières.
bursts = np.zeros(n)
for _ in range(9):
    c = rng.uniform(0.2, 4.5)
    w = rng.uniform(0.25, 0.9)
    bursts += rng.uniform(0.4, 1.0) * np.exp(-((t - c) / w) ** 2)
env = np.minimum(t / 0.05, 1.0) * np.exp(-t / 2.2) * (0.35 + bursts)
sound = crack * 0.6 + rumble * env
sound *= np.minimum(1.0, (7.0 - t) / 0.5)
sound /= np.abs(sound).max() * 1.1
sf.write("assets/sounds/thunder.ogg", np.stack([sound, sound], 1).astype(np.float32), RATE, format="OGG", subtype="VORBIS")
print("assets/sounds/thunder.ogg")
