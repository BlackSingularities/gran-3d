# GRAŃ — atlas szlaków górskich 3D

Trójwymiarowy atlas polskich gór działający w całości w przeglądarce. Łączy rzeczywisty numeryczny model terenu z siecią znakowanych szlaków z OpenStreetMap i dokłada do tego narzędzia analityczne: wysokość względną, wyznaczanie tras z wariantami, pole widoczności, linię wzroku, cienie o dowolnej porze dnia i przelot kamery nad trasą.

Regiony: **Tatry**, **Karkonosze**, **Pieniny**, **Babia Góra**, **Bieszczady**. Aplikacja nie korzysta z globalnej bazy terenu: dla każdego pasma dane są wycinane raz i zapisywane w repozytorium (`public/data/<region>/`).

## Uruchomienie

```bash
npm install
npm run dev        # http://localhost:5190
npm run build      # statyczna wersja w dist/ (działa z dowolnego hostingu plików)
```

## Funkcje

### Teren
- Siatka pełnej rozdzielczości modelu (Tatry: 1749×937 węzłów, ~25 m), normalne liczone w shaderze bezpośrednio z tekstury wysokości.
- **Realistyczne cieniowanie**: piętra roślinności (regiel, kosodrzewina, hale, turnie) zależne od wysokości, nachylenia i ekspozycji; **rzeczywiste pokrycie terenu z OSM** (stawy, lasy, kosodrzewina, piargi) wypiekane do maski rastrowej; mikrorzeźba proceduralna na skałach; odbicia nieba i odblaski słońca na wodzie.
- **Słońce według daty i godziny** (algorytm NOAA), **cienie rzucane** liczone na GPU przez śledzenie promieni w modelu wysokości, okluzja nieba (sky‑view factor), perspektywa powietrzna.
- **Sezonowość**: granica śniegu zależna od daty (dłużej na stokach północnych i w żlebach), barwy jesieni w reglu dolnym.
- Style: teren, **Sentinel‑2 cloudless** (mozaika satelitarna dopasowana do siatki), mapa turystyczna, hipsometria, **klasy nachylenia jak na mapach lawinowych**, ekspozycja stoków.
- Poziomice z automatycznym cięciem (10/20/50/100 m), siatka kilometrowa, przewyższenie pionowe ×1–3, przekrój geologiczny na bokach bryły.

### Trasy
- Graf szlaków zbudowany z relacji `route=hiking` OSM (kolory PTTK z `osmc:symbol`), wysokości próbkowane z DEM co 8–12 m.
- **Warianty naraz**: najszybsza, najkrótsza (dystans rzeczywisty 3D), najmniej podejść, najłatwiejsza (unika odcinków T4–T6) oraz objazdy wyznaczane metodą kar — zbieżne warianty są łączone z wieloma etykietami.
- Dowolna liczba punktów pośrednich, przeciągane znaczniki, punkty przyklejane do szlaków w dowolnym miejscu krawędzi.
- **Trasa przez teren**: A* po siatce DEM (ruchy 16‑kierunkowe) z funkcją Toblera i limitem nachylenia — do analizy rzeźby i porównania ze szlakiem.
- Metryki: dystans w rzucie i **rzeczywisty (po powierzchni)**, linia prosta i krętość, suma podejść/zejść (z histerezą przeciw szumowi DEM), czas wg **reguły PTTK** lub **DIN 33466** z tempem i korektą za skalę trudności SAC, **punkty GOT PTTK**, maks. nachylenie, ocena trudności.
- **Profil wysokości** kolorowany nachyleniem, z pasem kolorów szlaków, nazwami mijanych szczytów/przełęczy/schronisk, wariantami w tle i synchronizacją kursora z mapą.
- **Przelot kamery** wzdłuż trasy, eksport **GPX**, import GPX (przeciągnij plik na mapę), link z zapisanym widokiem i punktami trasy.

### Analizy
- **Wysokość względna**: dowolny punkt staje się poziomem 0 — teren w skali rozbieżnej, izolinia „tej samej wysokości”, pierścienie odległości, odsetek terenu wyżej/niżej, szczyty z Δh; w odczycie kursora Δh, odległość, kąt i azymut.
- **Pole widoczności** (viewshed) z uwzględnieniem krzywizny Ziemi i refrakcji, wysokość obserwatora 1,7/10/30/100 m, powierzchnia widoczna i lista widocznych szczytów.
- **Pomiar po terenie** i **linia wzroku** między dwoma punktami z miejscem przeszkody na profilu.
- **Panorama** z dowolnego miejsca (kamera na wysokości oczu, etykiety z odległościami).

### Interfejs
Wyszukiwarka (`/`) szczytów, schronisk, przełęczy i szlaków (także po kolorze), menu kontekstowe pod prawym przyciskiem, skróty klawiszowe (`?`), kompas, podziałka, odczyt kursora (współrzędne, wysokość, nachylenie, ekspozycja), adaptacyjna rozdzielczość renderowania.

## Dane

Dane są przygotowane skryptem `scripts/bake.mjs` i zapisane w repozytorium:

| plik | zawartość | źródło |
| --- | --- | --- |
| `dem.bin` | wysokości `uint16` (dm) w siatce Web Mercator | kafle **Terrarium** (Mapzen / AWS Open Data: SRTM, EU‑DEM i in.) |
| `meta.json` | wymiary i położenie siatki | — |
| `trails.json` | graf szlaków: węzły, krawędzie z geometrią i wysokościami, relacje | **© OpenStreetMap** (ODbL) przez Overpass API |
| `pois.json` | szczyty, przełęcze, schroniska, stawy, wodospady, jaskinie | **© OpenStreetMap** (ODbL) |
| `landcover.png` | maska R woda / G las / B skały (255) i zarośla (110), 2× gęstsza od DEM | **© OpenStreetMap** (ODbL) |

Obraz satelitarny ładowany na żądanie: **Sentinel‑2 cloudless 2020 © EOX IT Services** (CC BY‑NC‑SA 4.0), dane Copernicus.

Odświeżenie danych (np. po zmianach w OSM) albo dodanie regionu w `regions.json`:

```bash
npm run bake            # wszystkie regiony
npm run bake -- tatry   # jeden region
```

Pobrane kafle i odpowiedzi Overpass są buforowane w `.cache/`.

## Architektura

```
src/
  core/      dem.ts (siatka, próbkowanie, raycasting), graph.ts (graf szlaków, Dijkstra, warianty),
             metrics.ts (czasy, GOT, profil), sun.ts (położenie Słońca), gpx.ts, store.ts
  scene/     engine.ts (kamera, sterowanie, przeloty), terrain.ts + shaders.ts (teren, przebiegi GPU),
             trails.ts, routes.ts, overlay.ts (etykiety i znaczniki HTML)
  workers/   terrain.worker.ts (widoczność, A* po terenie)
  ui/        panel.ts, chrome.ts, profile.ts, icons.ts
```

Uwaga: model terenu o rozdzielczości ~25 m wygładza najostrzejsze granie (np. Gerlach 2655 m ma w modelu ~2590 m) — nazwy i wysokości szczytów pochodzą z OSM, a różnicę widać w karcie szczytu. Trasy przez teren to analiza rzeźby, a nie zachęta do schodzenia ze szlaków w parkach narodowych.
