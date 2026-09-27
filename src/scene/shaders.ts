// Shadery terenu GRAŃ. Wysokości czytane bezpośrednio z tekstury DEM (R32F) z ręczną interpolacją,
// dzięki czemu cieniowanie ma pełną rozdzielczość modelu niezależnie od gęstości siatki.

export const DEM_GLSL = /* glsl */ `
uniform highp sampler2D uDem;
uniform vec2 uSize;
uniform float uMpp;

float hAt(vec2 g) {
  g = clamp(g, vec2(0.0), uSize - 1.001);
  ivec2 i = ivec2(floor(g));
  vec2 f = g - vec2(i);
  float a = texelFetch(uDem, i, 0).r;
  float b = texelFetch(uDem, i + ivec2(1, 0), 0).r;
  float c = texelFetch(uDem, i + ivec2(0, 1), 0).r;
  float d = texelFetch(uDem, i + ivec2(1, 1), 0).r;
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}
`;

export const NOISE_GLSL = /* glsl */ `
float hash12(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}
float vnoise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash12(i), hash12(i + vec2(1, 0)), u.x), mix(hash12(i + vec2(0, 1)), hash12(i + vec2(1, 1)), u.x), u.y);
}
float fbm(vec2 p) {
  float s = 0.0, a = 0.5;
  for (int i = 0; i < 5; i++) { s += a * vnoise(p); p = p * 2.03 + vec2(17.1, 3.7); a *= 0.5; }
  return s;
}
float fbm3(vec2 p) {
  float s = 0.0, a = 0.5;
  for (int i = 0; i < 3; i++) { s += a * vnoise(p); p = p * 2.07 + vec2(5.3, 11.9); a *= 0.5; }
  return s;
}
`;

export const terrainVertex = /* glsl */ `
#include <common>
#include <logdepthbuf_pars_vertex>
attribute vec2 grid;
varying vec2 vGrid;
varying vec3 vPos;
varying vec3 vWorld;
varying float vDist;
void main() {
  vGrid = grid;
  vPos = position;
  vec4 wp = modelMatrix * vec4(position, 1.0);
  vWorld = wp.xyz;
  vec4 mv = viewMatrix * wp;
  vDist = -mv.z;
  gl_Position = projectionMatrix * mv;
  #include <logdepthbuf_vertex>
}
`;

export const tileVertex = /* glsl */ `
#include <common>
#include <logdepthbuf_pars_vertex>
attribute vec2 tpos;
attribute float skirt;
uniform highp sampler2D uTile;
uniform vec4 uTileInfo;      // początek x, początek z, metry na próbkę, głębokość fartucha
uniform float uMpp;
uniform vec2 uSize;
varying vec2 vGrid;
varying vec3 vPos;
varying vec3 vWorld;
varying float vDist;
varying vec2 vTexel;
void main() {
  float h = texelFetch(uTile, ivec2(tpos) + 1, 0).r - skirt * uTileInfo.w;
  vec3 p = vec3(uTileInfo.x + tpos.x * uTileInfo.z, h, uTileInfo.y + tpos.y * uTileInfo.z);
  vPos = p;
  vTexel = tpos + 1.0;
  vGrid = vec2(p.x / uMpp + (uSize.x - 1.0) * 0.5, p.z / uMpp + (uSize.y - 1.0) * 0.5);
  vec4 wp = modelMatrix * vec4(p, 1.0);
  vWorld = wp.xyz;
  vec4 mv = viewMatrix * wp;
  vDist = -mv.z;
  gl_Position = projectionMatrix * mv;
  #include <logdepthbuf_vertex>
}
`;

export const terrainFragment = /* glsl */ `
#include <common>
#include <logdepthbuf_pars_fragment>
${DEM_GLSL}
${NOISE_GLSL}

uniform float uExag;
uniform vec2 uMinMax;
uniform int uStyle;          // 0 teren, 1 hipsometria, 2 nachylenie, 3 ekspozycja, 4 satelita, 5 mapa
uniform sampler2D uShadowTex;
uniform sampler2D uAoTex;
uniform sampler2D uSatTex;
uniform float uSatReady;
uniform sampler2D uVsTex;
uniform float uVsOn;
uniform float uContourOn;
uniform float uContourInt;
uniform float uShadowOn;
uniform vec3 uSunDir;
uniform vec3 uSunColor;
uniform vec3 uSkyColor;
uniform vec3 uFogColor;
uniform float uFogDensity;
uniform float uRelOn;
uniform vec3 uRef;           // x, z świata, wysokość
uniform float uRelRange;
uniform vec4 uCursor;        // x, z, wysokość, aktywny
uniform float uLens;         // soczewka: 0 brak, 1 poziomica, 2 ta sama wysokość, 3 względna, 4 widoczność
uniform float uLensOn;       // czy jest punkt źródłowy (kursor lub przypięty)
uniform float uBandTol;      // połowa szerokości pasa „tej samej wysokości” [m]
uniform float uGridOn;
uniform float uSnowLine;
uniform float uSnowOn;
uniform float uAutumn;
uniform vec3 uBiome;         // las, kosodrzewina, skały
uniform sampler2D uLand;     // R woda, G las, B skały (255) / zarośla (~110)
uniform float uLandOn;
uniform float uNoiseAmt;
#ifdef TILE
uniform highp sampler2D uTile;
uniform vec4 uTileInfo;
uniform sampler2D uOrtho;
uniform vec4 uOrthoRect;
uniform float uOrthoOn;
varying vec2 vTexel;
float hTile(vec2 t) {
  t = clamp(t, vec2(0.0), vec2(257.999));
  ivec2 i = ivec2(floor(t));
  vec2 f = t - vec2(i);
  float a = texelFetch(uTile, i, 0).r;
  float b = texelFetch(uTile, i + ivec2(1, 0), 0).r;
  float c = texelFetch(uTile, i + ivec2(0, 1), 0).r;
  float d = texelFetch(uTile, i + ivec2(1, 1), 0).r;
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}
#define HS hTile
#else
#define HS hAt
#endif

varying vec2 vGrid;
varying vec3 vPos;
varying vec3 vWorld;
varying float vDist;

vec3 srgb(vec3 c) { return pow(c, vec3(2.2)); }

vec3 hypso(float e) {
  const int N = 10;
  float st[10] = float[10](150.0, 400.0, 650.0, 900.0, 1150.0, 1400.0, 1700.0, 2000.0, 2300.0, 2600.0);
  vec3 cs[10] = vec3[10](
    vec3(0.40, 0.62, 0.40), vec3(0.55, 0.73, 0.45), vec3(0.76, 0.82, 0.52), vec3(0.93, 0.89, 0.60),
    vec3(0.94, 0.79, 0.52), vec3(0.87, 0.64, 0.41), vec3(0.74, 0.49, 0.32), vec3(0.60, 0.40, 0.31),
    vec3(0.66, 0.60, 0.60), vec3(0.97, 0.97, 0.98));
  if (e <= st[0]) return srgb(cs[0]);
  for (int i = 1; i < N; i++) {
    if (e < st[i]) return srgb(mix(cs[i - 1], cs[i], (e - st[i - 1]) / (st[i] - st[i - 1])));
  }
  return srgb(cs[N - 1]);
}

vec3 hsv2rgb(vec3 c) {
  vec3 p = abs(fract(c.xxx + vec3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
  return c.z * mix(vec3(1.0), clamp(p - 1.0, 0.0, 1.0), c.y);
}

float contourLine(float v, float interval, float width) {
  float f = v / interval;
  float df = fwidth(f);
  float d = abs(fract(f - 0.5) - 0.5) / max(df, 1e-5);
  float a = 1.0 - smoothstep(width - 0.5, width + 0.5, d);
  return a * (1.0 - smoothstep(0.18, 0.45, df));
}

void main() {
  #include <logdepthbuf_fragment>
#ifdef TILE
  // kafle wystają poza obszar modelu – odcinamy je na krawędzi bryły
  if (vGrid.x < -0.5 || vGrid.y < -0.5 || vGrid.x > uSize.x - 0.5 || vGrid.y > uSize.y - 0.5) discard;
  vec2 P = vTexel;
  float stepM = uTileInfo.z;
#else
  vec2 P = vGrid;
  float stepM = uMpp;
#endif
  float elev = HS(P);
  float hl = HS(P - vec2(1.0, 0.0)), hr = HS(P + vec2(1.0, 0.0));
  float hu = HS(P - vec2(0.0, 1.0)), hd = HS(P + vec2(0.0, 1.0));
  vec2 grad = vec2(hr - hl, hd - hu) / (2.0 * stepM);
  // do klasyfikacji (skała/las/piarg) nachylenie uśrednione do ~12 m – spójne między poziomami kafli
  vec2 gradC = grad;
#ifdef TILE
  float cs = max(1.0, 12.0 / stepM);
  gradC = vec2(HS(P + vec2(cs, 0.0)) - HS(P - vec2(cs, 0.0)), HS(P + vec2(0.0, cs)) - HS(P - vec2(0.0, cs))) / (2.0 * cs * stepM);
#endif
  float slope = atan(length(gradC));                // nachylenie do klasyfikacji [rad]
  float slopeFine = atan(length(grad));
  float slopeDeg = degrees(slope);
  float aspect = atan(-gradC.x, gradC.y);           // azymut spadku [-pi, pi], 0 = N
  float northness = cos(aspect);
  vec3 N = normalize(vec3(-grad.x * uExag, 1.0, -grad.y * uExag));
  vec2 uv = (vGrid + 0.5) / uSize;
  vec2 wp = vPos.xz;
  float near = 1.0 - smoothstep(1500.0, 14000.0, vDist);

  // --- mikrorzeźba: szum proceduralny wzmacniany na skałach ---
  float nBig = fbm(wp / 420.0);
  float nMid = fbm(wp / 70.0);
  float rockMask = smoothstep(0.62, 0.82, slope) + smoothstep(uBiome.z - 120.0, uBiome.z + 120.0, elev + nBig * 200.0) * 0.8;
  rockMask = clamp(rockMask, 0.0, 1.0);
  if (near > 0.0 && (uStyle == 0 || uStyle == 4)) {
    float s = 2.0;
    vec2 p = wp / 16.0;
    float n0 = fbm3(p), nx = fbm3(p + vec2(s / 16.0, 0.0)), nz = fbm3(p + vec2(0.0, s / 16.0));
    vec3 dn = vec3(-(nx - n0), 0.0, -(nz - n0)) * (0.6 + 2.2 * rockMask) * near * uNoiseAmt;
    N = normalize(N + dn * 1.3);
  }

  vec3 landC = uLandOn > 0.5 ? texture(uLand, uv).rgb : vec3(0.0);
  float water = smoothstep(0.4, 0.62, landC.r);
  vec3 base;
  float orthoA = 0.0;
  float lit = 1.0; // 1 = oświetlenie słoneczne, 0 = kartograficzne
  float snow = 0.0;
  if (uStyle == 0 || uStyle == 4) {
    // ---------- realistyczny teren: piętra roślinności ----------
    float warm = -northness * 70.0;               // stoki południowe: wyższa granica lasu
    float treeline = uBiome.x + warm + (nBig - 0.5) * 220.0;
    float shrubline = uBiome.y + warm + (nMid - 0.5) * 160.0;
    vec3 meadowLow = mix(srgb(vec3(0.44, 0.52, 0.26)), srgb(vec3(0.58, 0.58, 0.34)), vnoise(wp / 260.0));
    vec3 beech = mix(srgb(vec3(0.20, 0.29, 0.12)), srgb(vec3(0.55, 0.33, 0.12)), uAutumn * smoothstep(0.35, 0.7, vnoise(wp / 90.0)));
    vec3 spruce = srgb(vec3(0.10, 0.17, 0.10)) * (0.8 + 0.4 * vnoise(wp / 9.0));
    vec3 forest = mix(beech, spruce, smoothstep(900.0, 1150.0, elev + (nMid - 0.5) * 200.0));
    forest *= 0.85 + 0.3 * vnoise(wp / 6.0) * near;
    vec3 pine = mix(srgb(vec3(0.17, 0.25, 0.11)), srgb(vec3(0.24, 0.31, 0.14)), vnoise(wp / 12.0));
    vec3 alpine = mix(srgb(vec3(0.46, 0.47, 0.28)), srgb(vec3(0.58, 0.45, 0.26)), uAutumn);
    alpine = mix(alpine, srgb(vec3(0.52, 0.52, 0.40)), vnoise(wp / 40.0) * 0.5);
    vec3 rock = mix(srgb(vec3(0.44, 0.43, 0.41)), srgb(vec3(0.58, 0.56, 0.52)), fbm3(wp / 25.0));
    rock = mix(rock, srgb(vec3(0.36, 0.33, 0.30)), smoothstep(0.9, 1.2, slope) * 0.6);
    vec3 scree = srgb(vec3(0.62, 0.60, 0.56));

    float rockA = clamp(smoothstep(0.6, 0.8, slope) * smoothstep(treeline - 300.0, treeline, elev) + smoothstep(0.85, 1.05, slope), 0.0, 1.0);
    rockA = max(rockA, smoothstep(uBiome.z - 80.0, uBiome.z + 160.0, elev + (nMid - 0.5) * 260.0) * smoothstep(0.25, 0.45, slope + nMid * 0.2));
    float screeA = smoothstep(0.45, 0.6, slope) * (1.0 - smoothstep(0.7, 0.85, slope)) * smoothstep(uBiome.y, uBiome.z, elev) * vnoise(wp / 50.0);
    if (uLandOn > 0.5) {
      // rzeczywiste pokrycie terenu z OpenStreetMap
      float edge = (vnoise(wp / 22.0) - 0.5) * 0.35;
      float forestM = smoothstep(0.3, 0.7, landC.g + edge);
      float rockM = smoothstep(0.62, 0.9, landC.b + edge * 0.5);
      float scrubM = smoothstep(0.22, 0.4, landC.b + edge * 0.6) * (1.0 - rockM);
      vec3 open = mix(meadowLow, alpine, smoothstep(treeline - 250.0, treeline + 50.0, elev));
      base = mix(open, forest, forestM);
      base = mix(base, pine, scrubM * (1.0 - forestM));
      base = mix(base, mix(scree, rock, smoothstep(0.55, 0.8, slope)), max(rockM, rockA * (1.0 - forestM) * 0.85));
    } else {
      // łąki w dolinach (łagodne stoki poniżej lasu)
      float valley = (1.0 - smoothstep(0.08, 0.2, slope)) * (1.0 - smoothstep(treeline - 500.0, treeline - 250.0, elev));
      float clearings = smoothstep(0.58, 0.66, vnoise(wp / 380.0 + 3.1)) * (1.0 - smoothstep(0.25, 0.4, slope));
      base = mix(forest, meadowLow, clamp(valley * 0.85 + clearings * 0.9, 0.0, 1.0));
      float t1 = smoothstep(treeline - 40.0, treeline + 40.0, elev);
      base = mix(base, mix(pine, alpine, smoothstep(0.35, 0.7, vnoise(wp / 55.0))), t1);
      float t2 = smoothstep(shrubline - 60.0, shrubline + 60.0, elev);
      base = mix(base, alpine, t2);
      base = mix(base, rock, rockA);
      base = mix(base, scree, screeA * 0.6);
    }

    if (uStyle == 4 && uSatReady > 0.5) {
      vec3 sat = texture(uSatTex, uv).rgb;
      base = srgb(sat) * 1.15;
    }
#ifdef TILE
    if (uStyle == 4 && uOrthoOn > 0.5) {
      vec4 o = texture(uOrtho, uOrthoRect.xy + uOrthoRect.zw * ((vTexel - 1.0) / 256.0));
      // rozjaśnienie głębokich cieni wypalonych w zdjęciu lotniczym
      vec3 oc = pow(o.rgb, vec3(0.82)) * 1.04;
      base = mix(base, srgb(oc), o.a);
      orthoA = o.a;
    }
#endif

    // ---------- śnieg sezonowy ----------
    if (uSnowOn > 0.5 && orthoA < 0.5) {
      float line = uSnowLine + northness * -140.0 + (nMid - 0.5) * 260.0;
      snow = smoothstep(line - 60.0, line + 80.0, elev) * (1.0 - smoothstep(0.85, 1.0, slope));
      // żleby i zacienione kotły trzymają śnieg dłużej
      float gully = smoothstep(0.5, 0.9, northness) * smoothstep(0.35, 0.6, slope) * smoothstep(line - 450.0, line - 100.0, elev) * step(0.55, vnoise(wp / 35.0));
      snow = max(snow, gully * 0.9);
      base = mix(base, srgb(vec3(0.93, 0.95, 0.98)), snow);
    }
  } else if (uStyle == 1) {
    base = hypso(elev);
    lit = 0.0;
  } else if (uStyle == 2) {
    vec3 c = srgb(vec3(0.94, 0.93, 0.89));
    if (slopeDeg >= 45.0) c = srgb(vec3(0.33, 0.18, 0.46));
    else if (slopeDeg >= 40.0) c = srgb(vec3(0.63, 0.20, 0.55));
    else if (slopeDeg >= 35.0) c = srgb(vec3(0.89, 0.18, 0.15));
    else if (slopeDeg >= 30.0) c = srgb(vec3(0.98, 0.56, 0.12));
    else if (slopeDeg >= 25.0) c = srgb(vec3(0.99, 0.87, 0.30));
    else if (slopeDeg >= 15.0) c = srgb(vec3(0.84, 0.90, 0.72));
    base = c;
    lit = 0.0;
  } else if (uStyle == 3) {
    float hue = fract(aspect / 6.2831853 + 1.0);
    float sat = clamp(slopeDeg / 30.0, 0.0, 1.0) * 0.8;
    base = srgb(hsv2rgb(vec3(hue, sat, 0.93)));
    lit = 0.0;
  } else {
    // mapa turystyczna: las na zielono, wyżej krem, skały szare
    float treeline = uBiome.x - northness * 70.0 + (nBig - 0.5) * 160.0;
    vec3 paper = srgb(vec3(0.97, 0.95, 0.90));
    vec3 wood = srgb(vec3(0.80, 0.89, 0.70));
    float woodA = uLandOn > 0.5 ? smoothstep(0.35, 0.65, landC.g) : 1.0 - smoothstep(treeline - 30.0, treeline + 30.0, elev);
    base = mix(paper, wood, woodA);
    float rk = smoothstep(0.75, 0.95, slope) * smoothstep(uBiome.x - 200.0, uBiome.x + 200.0, elev);
    base = mix(base, srgb(vec3(0.80, 0.79, 0.78)), rk);
    lit = 0.0;
  }

  // ---------- oświetlenie ----------
  float ao = texture(uAoTex, uv).r;
  vec3 col;
  if (lit > 0.5) {
    float ndl = max(dot(N, uSunDir), 0.0);
    float sh = uShadowOn > 0.5 ? texture(uShadowTex, uv).r : 1.0;
    float sky = 0.55 + 0.45 * N.y;
    vec3 direct = uSunColor * ndl * sh;
    vec3 ambient = uSkyColor * sky * (0.35 + 0.65 * ao);
    // odbite światło od stoków naprzeciwko
    vec3 bounce = base * uSunColor * 0.08 * (1.0 - N.y) * ao;
    vec3 light = direct + ambient;
    // zdjęcie lotnicze ma własne cienie – nasze oświetlenie tylko częściowo
    float flatL = dot(uSunColor, vec3(0.3, 0.59, 0.11)) * 0.28 + dot(uSkyColor, vec3(0.33)) * 0.95;
    light = mix(light, vec3(flatL), 0.55 * orthoA);
    col = base * light + bounce * (1.0 - orthoA);
    // połysk śniegu
    if (snow > 0.0) {
      vec3 V = normalize(cameraPosition - vWorld);
      vec3 Hh = normalize(uSunDir + V);
      col += uSunColor * pow(max(dot(N, Hh), 0.0), 40.0) * 0.25 * snow * sh;
    }
  } else {
    // cieniowanie kartograficzne: kilka źródeł z północnego zachodu + okluzja
    vec3 Nc = normalize(vec3(-grad.x * 1.6, 1.0, -grad.y * 1.6));
    float h1 = max(dot(Nc, normalize(vec3(-0.6, 0.7, -0.6))), 0.0);
    float h2 = max(dot(Nc, normalize(vec3(-0.9, 0.9, 0.1))), 0.0);
    float h3 = max(dot(Nc, normalize(vec3(0.0, 0.8, -0.8))), 0.0);
    float shade = h1 * 0.55 + h2 * 0.25 + h3 * 0.2;
    shade = mix(0.42, 1.12, shade) * (0.7 + 0.3 * ao);
    col = base * shade;
  }

  // ---------- woda: stawy, jeziora, rzeki ----------
  if (water > 0.0) {
    vec3 V = normalize(cameraPosition - vWorld);
    // zmarszczki tylko z bliska – z daleka szum dawał „piasek” (aliasing)
    float rip = (1.0 - smoothstep(600.0, 3500.0, vDist)) * 0.035;
    vec3 wn = normalize(vec3((vnoise(wp / 5.0 + uSunDir.xz * 3.0) - 0.5) * rip, 1.0, (vnoise(wp / 6.5 + 9.0) - 0.5) * rip));
    float cosV = max(dot(wn, V), 0.0);
    float fres = 0.02 + 0.98 * pow(1.0 - cosV, 5.0);
    // płycizny przy brzegu jaśniejsze (maska wody rośnie od brzegu do środka)
    float depth = smoothstep(0.62, 0.98, landC.r);
    vec3 shallow = srgb(vec3(0.16, 0.36, 0.34));
    vec3 deep = srgb(vec3(0.015, 0.075, 0.11));
    vec3 body = mix(shallow, deep, depth);
    vec3 wc;
    if (lit > 0.5) {
      float sh = uShadowOn > 0.5 ? texture(uShadowTex, uv).r : 1.0;
      // odbicie nieba: błękit zenitu przechodzący w jasny horyzont przy patrzeniu pod kątem
      vec3 skyRefl = mix(uSkyColor * 1.25, uFogColor * 1.05, pow(1.0 - cosV, 2.0));
      vec3 lightIn = uSkyColor * 0.9 + uSunColor * 0.18 * sh;
      wc = mix(body * lightIn, skyRefl, fres);
      vec3 Hh = normalize(uSunDir + V);
      float glint = pow(max(dot(wn, Hh), 0.0), 380.0) * (1.0 - smoothstep(1500.0, 8000.0, vDist));
      wc += uSunColor * glint * 2.2 * sh;
    } else {
      wc = uStyle == 5 ? srgb(vec3(0.62, 0.80, 0.93)) : srgb(vec3(0.36, 0.60, 0.80));
    }
    // ciemniejsza linia brzegowa
    float shore = smoothstep(0.45, 0.75, landC.r);
    col = mix(col, wc * mix(0.75, 1.0, shore), water);
  }

  // ---------- wysokość względna ----------
  if (uRelOn > 0.5) {
    float dh = elev - uRef.z;
    float t = clamp(dh / uRelRange, -1.0, 1.0);
    vec3 above = t < 0.5 ? mix(srgb(vec3(0.98, 0.94, 0.80)), srgb(vec3(0.97, 0.60, 0.22)), t * 2.0)
                         : mix(srgb(vec3(0.97, 0.60, 0.22)), srgb(vec3(0.62, 0.08, 0.10)), (t - 0.5) * 2.0);
    vec3 below = mix(srgb(vec3(0.82, 0.93, 0.95)), srgb(vec3(0.04, 0.22, 0.45)), -t);
    vec3 rc = dh >= 0.0 ? above : below;
    vec3 Nc = normalize(vec3(-grad.x * 1.4, 1.0, -grad.y * 1.4));
    float shade = mix(0.55, 1.1, max(dot(Nc, normalize(vec3(-0.6, 0.75, -0.5))), 0.0)) * (0.75 + 0.25 * ao);
    col = mix(col, rc * shade, 0.88);
    float band = contourLine(dh, uRelRange / 5.0, 0.6);
    col = mix(col, col * 0.45, band * 0.7);
    float zf = abs(dh) / max(fwidth(dh), 1e-4);
    float z0 = 1.0 - smoothstep(1.0, 2.6, zf);
    col = mix(col, vec3(1.0), z0 * 0.95);
    // pierścienie odległości co 1 km
    float rd = length(wp - uRef.xy);
    float ring = contourLine(rd, 1000.0, 0.5) * 0.35;
    col = mix(col, vec3(1.0), ring);
    col += vec3(1.0, 0.9, 0.7) * (1.0 - smoothstep(0.0, 60.0 + vDist * 0.004, rd)) * 0.6;
  }

  // ---------- poziomice ----------
  if (uContourOn > 0.5) {
    float ci = uContourInt;
    bool realistic = uStyle == 0 || uStyle == 4;
    float minor = realistic ? 0.0 : contourLine(elev, ci, 0.55);
    float major = contourLine(elev, ci * 5.0, realistic ? 0.8 : 1.05);
    vec3 cc = (uStyle == 0 || uStyle == 4) ? vec3(0.96, 0.92, 0.82) : srgb(vec3(0.55, 0.33, 0.16));
    float ca = (realistic ? 0.0 : 0.5) * (1.0 - water);
    col = mix(col, realistic ? col * 0.6 + cc * 0.12 : cc * 0.9, clamp(minor * ca + major * (realistic ? 0.35 : ca + 0.2), 0.0, 1.0));
  }

  // ---------- siatka kilometrowa ----------
  if (uGridOn > 0.5) {
    float gx = contourLine(wp.x, 1000.0, 0.5), gz = contourLine(wp.y, 1000.0, 0.5);
    col = mix(col, vec3(0.95, 0.9, 0.8), max(gx, gz) * 0.35);
  }

  // ---------- widoczność ----------
  if (uVsOn > 0.5) {
    float v = texture(uVsTex, uv).r;
    float vis = smoothstep(0.0, 0.02, v);
    vec3 lum = vec3(dot(col, vec3(0.3, 0.59, 0.11)));
    vec3 hidden = mix(col, lum, 0.8) * 0.26;
    float hatch = step(0.5, fract((wp.x + wp.y) / (18.0 + vDist * 0.004)));
    hidden *= 0.85 + 0.25 * hatch;
    vec3 shown = col * vec3(1.18, 1.08, 0.9) + vec3(0.07, 0.045, 0.0);
    col = mix(hidden, shown, vis);
    float edge = clamp(fwidth(vis) * 3.0, 0.0, 1.0);
    col = mix(col, vec3(1.0, 0.78, 0.35), edge);
  }

  // ---------- kursor: pierścień + poziomica przez kursor ----------
  if (uCursor.w > 0.5) {
    float rd = length(wp - uCursor.xy);
    float r = 40.0 + vDist * 0.012;
    float ring = 1.0 - smoothstep(0.0, 1.8, abs(rd - r) / max(fwidth(rd), 1e-4));
    col = mix(col, vec3(1.0, 0.95, 0.85), ring * 0.9);
  }

  // ---------- soczewka: poziomica przez punkt ----------
  if (uLensOn > 0.5 && uLens > 0.5 && uLens < 1.5) {
    float dz = elev - uRef.z;
    float isoF = abs(dz) / max(fwidth(elev), 1e-4);
    float core = 1.0 - smoothstep(0.7, 1.7, isoF);
    float glow = (1.0 - smoothstep(1.5, 7.0, isoF)) * 0.35;
    float fade = 1.0 - smoothstep(9000.0, 20000.0, length(wp - uRef.xy));
    col = mix(col, vec3(1.0, 0.86, 0.45), glow * fade);
    col = mix(col, vec3(1.0, 0.97, 0.88), core * fade);
  }

  // ---------- soczewka: ta sama wysokość (pas ±tolerancja) ----------
  if (uLensOn > 0.5 && uLens > 1.5 && uLens < 2.5) {
    float d = abs(elev - uRef.z);
    float inBand = 1.0 - smoothstep(uBandTol * 0.8, uBandTol, d);
    vec3 lum = vec3(dot(col, vec3(0.3, 0.59, 0.11)));
    vec3 outside = mix(col, lum, 0.55) * 0.55;
    vec3 inside = mix(col, srgb(vec3(0.25, 0.85, 0.92)), 0.55) * 1.15 + srgb(vec3(0.02, 0.08, 0.1));
    col = mix(outside, inside, inBand);
    // krawędzie pasa i linia dokładnie na poziomie
    float eF = abs(d - uBandTol) / max(fwidth(elev), 1e-4);
    col = mix(col, srgb(vec3(0.55, 0.98, 1.0)), (1.0 - smoothstep(0.6, 1.6, eF)) * 0.8);
    float cF = d / max(fwidth(elev), 1e-4);
    col = mix(col, vec3(1.0), (1.0 - smoothstep(0.6, 1.6, cF)) * 0.9);
  }

  // ---------- perspektywa powietrzna ----------
  float fog = 1.0 - exp(-pow(vDist * uFogDensity, 1.35));
  fog *= mix(1.0, 0.75, smoothstep(uMinMax.x, uMinMax.y, elev));
  col = mix(col, uFogColor, clamp(fog, 0.0, 0.92));

  gl_FragColor = vec4(col, 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}
`;

// ----- boki bryły terenu (przekrój geologiczny) -----
export const wallVertex = /* glsl */ `
#include <common>
#include <logdepthbuf_pars_vertex>
attribute float top;
varying float vY;
varying float vTop;
varying vec2 vXZ;
varying float vDist;
void main() {
  vY = position.y;
  vTop = top;
  vXZ = position.xz;
  vec4 wp = modelMatrix * vec4(position, 1.0);
  vec4 mv = viewMatrix * wp;
  vDist = -mv.z;
  gl_Position = projectionMatrix * mv;
  #include <logdepthbuf_vertex>
}
`;

export const wallFragment = /* glsl */ `
#include <common>
#include <logdepthbuf_pars_fragment>
${NOISE_GLSL}
uniform vec3 uFogColor;
uniform float uFogDensity;
uniform float uBase;
uniform vec3 uLight;
varying float vY;
varying float vTop;
varying vec2 vXZ;
varying float vDist;
void main() {
  #include <logdepthbuf_fragment>
  float depth = vTop - vY;
  float along = vXZ.x + vXZ.y;

  // Warstwy nie są idealnie poziome: szerokie fałdy, lokalne pofalowanie
  // oraz przesunięcia bloków dają efekt naturalnego przekroju skalnego.
  float fold = (fbm(vec2(along / 1800.0, 1.7)) - 0.5) * 150.0
             + sin(along / 720.0) * 22.0;
  float faultAxis = along + vY * 0.22 + (fbm(vec2(along / 900.0, vY / 700.0)) - 0.5) * 150.0;
  float faultCell = floor((faultAxis + 24000.0) / 2600.0);
  float faultShift = (hash12(vec2(faultCell, 6.2)) - 0.5) * 105.0;
  float rockY = vY + fold + faultShift;
  float layer = rockY / 58.0 + sin(rockY / 185.0) * 0.42
              + (fbm(vec2(rockY / 330.0, along / 1350.0)) - 0.5) * 0.34;
  float band = fract(layer);
  float id = floor(layer);
  // Kilka cienkich ław tworzy jedną formację; jej miąższość zmienia się
  // powoli, dzięki czemu przekrój nie wygląda jak równy stos desek.
  float formation = floor(layer * 0.38 + sin(layer * 0.23) * 0.62);
  float kind = hash12(vec2(formation, 3.0));

  vec3 limestone = pow(vec3(0.48, 0.47, 0.43), vec3(2.2));
  vec3 sandstone = pow(vec3(0.49, 0.38, 0.28), vec3(2.2));
  vec3 shale = pow(vec3(0.27, 0.29, 0.30), vec3(2.2));
  vec3 marl = pow(vec3(0.39, 0.41, 0.38), vec3(2.2));
  vec3 col = mix(limestone, sandstone, smoothstep(0.18, 0.62, kind));
  col = mix(col, shale, smoothstep(0.72, 0.9, kind));
  col = mix(col, marl, 0.22 * hash12(vec2(id, 17.0)));

  // Ciemniejsze styki, drobne laminacje i ziarnistość skały.
  float contactWeight = 0.45 + 0.55 * hash12(vec2(id, 29.0));
  float contact = smoothstep(0.02, 0.1 + 0.05 * contactWeight, band)
                * (1.0 - smoothstep(0.9 - 0.04 * contactWeight, 0.99, band));
  float lamina = 0.5 + 0.5 * sin(rockY * 0.34 + fbm(vec2(along / 130.0, rockY / 95.0)) * 5.0);
  float grain = hash12(floor(vec2(along * 0.28, vY * 0.28)));
  col *= (0.74 + 0.26 * mix(1.0, contact, contactWeight)) * (0.91 + 0.055 * lamina + 0.07 * grain);

  // Uskoki oraz miejscowe pionowe spękania przecinające osady.
  float faultPos = fract((faultAxis + 24000.0) / 2600.0);
  float faultLine = 1.0 - smoothstep(0.0, 0.012, min(faultPos, 1.0 - faultPos));
  float crackCoord = fract(along / 430.0 + fbm(vec2(vY / 210.0, along / 760.0)) * 0.24);
  float crack = 1.0 - smoothstep(0.008, 0.028, min(crackCoord, 1.0 - crackCoord));
  col *= 1.0 - faultLine * 0.34 - crack * 0.11 * smoothstep(45.0, 180.0, depth);

  // Próchniczna gleba i zwietrzelina tuż pod powierzchnią terenu.
  float soilNoise = fbm(vec2(along / 42.0, vY / 28.0));
  float soilDepth = depth + (soilNoise - 0.5) * 15.0;
  vec3 soil = pow(vec3(0.22, 0.17, 0.115), vec3(2.2)) * (0.78 + soilNoise * 0.34);
  col = mix(soil, col, smoothstep(7.0, 38.0, soilDepth));

  // Skała staje się chłodniejsza i ciemniejsza w głębi bryły.
  col *= 1.0 - 0.34 * smoothstep(90.0, 650.0, depth);
  col *= 0.9 + 0.1 * smoothstep(uBase, uBase + 700.0, vY);
  col *= uLight;
  float fog = 1.0 - exp(-pow(vDist * uFogDensity, 1.35));
  col = mix(col, uFogColor, clamp(fog, 0.0, 0.92));
  gl_FragColor = vec4(col, 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}
`;

// ----- przebiegi obliczeniowe na GPU: cienie rzucane i okluzja nieba -----
export const passVertex = /* glsl */ `
void main() { gl_Position = vec4(position.xy, 0.0, 1.0); }
`;

export const shadowPassFragment = /* glsl */ `
precision highp float;
${DEM_GLSL}
uniform vec3 uSun;
void main() {
  vec2 g = gl_FragCoord.xy - 0.5;
  float h0 = hAt(g) + 2.0;
  float horiz = length(uSun.xz);
  if (uSun.y <= -0.02) { gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0); return; }
  vec2 dir = uSun.xz / max(horiz, 1e-5);
  float tanA = uSun.y / max(horiz, 1e-5);
  float sh = 1.0;
  float t = 0.7;
  float k = 0.012; // półcień ~ średnica tarczy Słońca
  for (int i = 0; i < 110; i++) {
    vec2 p = g + dir * t;
    if (p.x < 0.0 || p.y < 0.0 || p.x > uSize.x - 1.0 || p.y > uSize.y - 1.0) break;
    float d = t * uMpp;
    float ray = h0 + d * tanA - d * d * 6.8e-8;
    float hh = hAt(p);
    sh = min(sh, clamp(((ray - hh) / d) / k + 0.5, 0.0, 1.0));
    if (sh <= 0.0) break;
    t = t * 1.045 + 0.6;
  }
  // miękkie przejście przy horyzoncie
  sh *= smoothstep(-0.02, 0.03, uSun.y);
  gl_FragColor = vec4(sh, sh, sh, 1.0);
}
`;

export const aoPassFragment = /* glsl */ `
precision highp float;
${DEM_GLSL}
void main() {
  vec2 g = gl_FragCoord.xy - 0.5;
  float h0 = hAt(g);
  float sum = 0.0;
  const int DIRS = 16;
  for (int a = 0; a < DIRS; a++) {
    float ang = float(a) / float(DIRS) * 6.2831853 + 0.2;
    vec2 dir = vec2(cos(ang), sin(ang));
    float maxS = 0.0;
    float t = 1.0;
    for (int i = 0; i < 22; i++) {
      vec2 p = g + dir * t;
      float d = t * uMpp;
      float s = (hAt(p) - h0) / d;
      maxS = max(maxS, s);
      t *= 1.32;
    }
    sum += 1.0 - sin(atan(maxS));
  }
  float v = sum / float(DIRS);
  gl_FragColor = vec4(v, v, v, 1.0);
}
`;
