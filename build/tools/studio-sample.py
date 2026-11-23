"""Przykładowa mapa Studio (Tatry, Hala Gąsienicowa) z prawdziwego terenu – do okna informacyjnego."""
import io, math, sys, urllib.request
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LightSource
from PIL import Image

Z = 13
W, E, S, N = 19.925, 20.045, 49.212, 49.262
OUT = sys.argv[1]

def tile_xy(lon, lat, z):
    n = 2 ** z
    x = (lon + 180) / 360 * n
    y = (1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * n
    return x, y

x0, y1 = tile_xy(W, S, Z); x1, y0 = tile_xy(E, N, Z)
tx = range(int(x0), int(x1) + 1); ty = range(int(y0), int(y1) + 1)
big = np.zeros((len(ty) * 256, len(tx) * 256))
for j, y in enumerate(ty):
    for i, x in enumerate(tx):
        url = f"https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{Z}/{x}/{y}.png"
        img = np.asarray(Image.open(io.BytesIO(urllib.request.urlopen(url).read())).convert("RGB")).astype(float)
        big[j*256:(j+1)*256, i*256:(i+1)*256] = img[..., 0] * 256 + img[..., 1] + img[..., 2] / 256 - 32768
px0 = int((x0 - tx[0]) * 256); px1 = int((x1 - tx[0]) * 256)
py0 = int((y0 - ty[0]) * 256); py1 = int((y1 - ty[0]) * 256)
dem = big[py0:py1, px0:px1]
print("min", dem.min(), "max", dem.max())
h, w = dem.shape
from scipy.ndimage import gaussian_filter  # noqa
dem = gaussian_filter(dem, 0.6)

lon = lambda c: W + (E - W) * c / w
def row_of(lat):
    _, y = tile_xy(W, lat, Z); return (y - y0) * 256
def col_of(lo):
    x, _ = tile_xy(lo, N, Z); return (x - x0) * 256
m_per_px = 40075016.686 * math.cos(math.radians((S + N) / 2)) / (256 * 2 ** Z)

PAPER = "#f4efe3"; BROWN = "#9a6a3c"; INK = "#2b2620"; BLUE = "#3f7fb5"; RED = "#b8322a"
fig = plt.figure(figsize=(12, 9.6), dpi=110, facecolor=PAPER)
ax = fig.add_axes([0.04, 0.27, 0.92, 0.66]); ax.set_facecolor(PAPER)
ls = LightSource(azdeg=315, altdeg=40)
shade = ls.hillshade(dem, vert_exag=1.2, dx=m_per_px, dy=m_per_px)
tint = np.interp(dem, [1000, 1500, 1800, 2300], [0.0, 0.15, 0.35, 0.55])
rgb = np.ones((h, w, 3)) * np.array([0.957, 0.937, 0.890])
rgb = rgb * (0.72 + 0.28 * shade[..., None]) - tint[..., None] * np.array([0.02, 0.05, 0.09])
ax.imshow(np.clip(rgb, 0, 1), extent=(0, w, h, 0), interpolation="bilinear")
lv = np.arange(900, 2400, 20)
ax.contour(dem, levels=[l for l in lv if l % 100], colors=BROWN, linewidths=0.35, alpha=0.75)
cs = ax.contour(dem, levels=[l for l in lv if l % 100 == 0], colors=BROWN, linewidths=0.9)
ax.clabel(cs, levels=[l for l in cs.levels if l % 200 == 0], fmt="%d", fontsize=6.5, inline_spacing=2, colors=BROWN)

# stawy (przybliżone obrysy jako płaskie miejsca przy znanych punktach)
for (la, lo_) in [(49.2297, 20.0181)]:
    r, c = row_of(la), col_of(lo_)
    ax.add_patch(plt.Circle((c, r), 7, color=BLUE, alpha=0.85, lw=0))

# szlak Kuźnice – Hala Gąsienicowa – Czarny Staw – Kościelec (schematycznie po punktach)
trail = [(49.2690, 19.9800), (49.2560, 19.9885), (49.2470, 19.9985), (49.2430, 20.0070), (49.2350, 20.0130), (49.2297, 20.0170), (49.2253, 20.0144)]
tr = [(col_of(lo_), row_of(la)) for la, lo_ in trail]
ax.plot([p[0] for p in tr], [p[1] for p in tr], color=RED, lw=1.6, ls=(0, (5, 2.5)))

peaks = [("Giewont", 49.2510, 19.9339, 1894), ("Kasprowy Wierch", 49.2318, 19.9817, 1987), ("Beskid", 49.2264, 19.9914, 2012),
         ("Świnica", 49.2194, 20.0094, 2301), ("Kościelec", 49.2253, 20.0144, 2155), ("Kopa Kondracka", 49.2368, 19.9516, 2005)]
for name, la, lo_, hm in peaks:
    r, c = int(row_of(la)), int(col_of(lo_))
    rr = slice(max(r-6, 0), r+7); cc = slice(max(c-6, 0), c+7)
    win = dem[rr, cc]; k = np.unravel_index(np.argmax(win), win.shape)
    pr, pc = rr.start + k[0], cc.start + k[1]
    if not (0 < pr < h and 0 < pc < w):
        continue
    ax.plot(pc, pr, marker="^", color=INK, ms=5)
    ax.text(pc + 5, pr - 4, f"{name}\n{hm}", fontsize=7.5, color=INK, va="bottom",
            fontweight="bold", linespacing=0.95)
ax.text(col_of(20.0080) + 6, row_of(49.2433), "Hala\nGąsienicowa", fontsize=7, style="italic", color="#3d5a2a")
ax.set_xlim(0, w); ax.set_ylim(h, 0); ax.set_xticks([]); ax.set_yticks([])
for s in ax.spines.values():
    s.set_color(INK); s.set_linewidth(1.2)
# podziałka 1 km i północ
km = 1000 / m_per_px
ax.plot([20, 20 + km], [h - 18, h - 18], color=INK, lw=3, solid_capstyle="butt")
ax.text(20 + km / 2, h - 24, "1 km", ha="center", fontsize=7.5, color=INK)
ax.annotate("N", xy=(w - 22, 18), xytext=(w - 22, 48), ha="center", fontsize=9, fontweight="bold", color=INK,
            arrowprops=dict(arrowstyle="-|>", color=INK, lw=1.2))

fig.text(0.04, 0.955, "Tatry · Hala Gąsienicowa", fontsize=15, fontweight="bold", color=INK)
fig.text(0.96, 0.958, "warstwice co 20 m · cieniowanie rzeźby · szlak", fontsize=8.5, color="#6b6255", ha="right")

# przekrój Kasprowy Wierch → Świnica → Kościelec
pa = [(49.2318, 19.9817), (49.2264, 19.9914), (49.2194, 20.0094), (49.2253, 20.0144)]
pts = []
for (la1, lo1), (la2, lo2) in zip(pa, pa[1:]):
    for t in np.linspace(0, 1, 120, endpoint=False):
        pts.append((la1 + (la2 - la1) * t, lo1 + (lo2 - lo1) * t))
pts.append(pa[-1])
from scipy.ndimage import map_coordinates
zz = map_coordinates(dem, [[row_of(p[0]) for p in pts], [col_of(p[1]) for p in pts]], order=1)
d = [0.0]
for a, b in zip(pts, pts[1:]):
    d.append(d[-1] + math.hypot((b[0]-a[0]) * 111320, (b[1]-a[1]) * 111320 * math.cos(math.radians(a[0]))))
d = np.array(d) / 1000
ax.plot([col_of(p[1]) for p in pa], [row_of(p[0]) for p in pa], color=INK, lw=0.8, ls=":")
ax.text(col_of(pa[0][1]) - 4, row_of(pa[0][0]) + 12, "A", fontsize=9, fontweight="bold", color=INK, ha="right")
ax.text(col_of(pa[-1][1]) + 4, row_of(pa[-1][0]) + 12, "B", fontsize=9, fontweight="bold", color=INK)
bx = fig.add_axes([0.07, 0.06, 0.89, 0.15]); bx.set_facecolor(PAPER)
bx.fill_between(d, zz, 1600, color="#d9ccb2"); bx.plot(d, zz, color=INK, lw=1.2)
bx.set_ylim(1600, 2400); bx.set_xlim(0, d[-1])
bx.set_ylabel("m n.p.m.", fontsize=7.5, color=INK); bx.set_xlabel("km", fontsize=7.5, color=INK)
bx.tick_params(labelsize=7, colors=INK); bx.grid(color="#cbbfa6", lw=0.5)
for s in bx.spines.values():
    s.set_color(INK); s.set_linewidth(0.8)
fig.text(0.07, 0.225, "Przekrój A–B: Kasprowy Wierch – Świnica – Kościelec", fontsize=9.5, fontweight="bold", color=INK)
fig.text(0.96, 0.012, "Przykład · dane wysokości: Mapzen Terrain Tiles (SRTM, EU-DEM)", fontsize=6.5, color="#6b6255", ha="right")
buf = io.BytesIO(); fig.savefig(buf, format="png", facecolor=PAPER)
im = Image.open(buf).convert("RGB")
im = im.resize((1100, int(im.height * 1100 / im.width)), Image.LANCZOS)
im = im.quantize(colors=256, method=Image.Quantize.FASTOCTREE, dither=Image.Dither.NONE)
im.save(OUT, "PNG", optimize=True)
print(OUT, im.size)
