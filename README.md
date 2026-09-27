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
- **Model LiDAR ~3 m** (Tatry, Pieniny, Babia Góra) i **~6 m** (Karkonosze, Bieszczady) z darmowych danych krajowych: **NMT GUGiK** (Polska, lotnicze skanowanie laserowe) i **DMR 5G ČÚZK** (Czechy). Poza ich zasięgiem (Słowacja, Ukraina) – model globalny z płynnym przejściem na granicy.
- **Teren kaflowy z poziomami szczegółowości** (drzewo czwórkowe Web Mercator, kafle 259×259 z ramką, fartuchy maskujące szczeliny), doczytywany w wątkach roboczych wokół kamery; suwak szczegółowości w zakładce „Mapa i światło”.
- **Ortofotomapa** z usług krajowych składana na każdy kafel: **GUGiK** (PL), **ZBGIS** (SK), **ČÚZK** (CZ) – do ~0,8 m/px; w tle mozaika Sentinel‑2.
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
- **Soczewka kursora** (panel pod narzędziami, klawisze `H` `W` `O`): teren na żywo pokazuje wokół kursora **tę samą wysokość** – jako poziomicę lub pas ±5–50 m, **wysokość względną** albo **widoczność** z miejsca pod kursorem. Kliknięcie przypina soczewkę w punkcie (panel pokazuje wtedy statystyki i listy szczytów), `Esc` ją odpina.
- **Wysokość względna**: dowolny punkt staje się poziomem 0 — teren w skali rozbieżnej, izolinia „tej samej wysokości”, pierścienie odległości, odsetek terenu wyżej/niżej, szczyty z Δh; w odczycie kursora Δh, odległość, kąt i azymut.
- **Pole widoczności** (viewshed) z uwzględnieniem krzywizny Ziemi i refrakcji, wysokość obserwatora 1,7/10/30/100 m, powierzchnia widoczna i lista widocznych szczytów.
- **Pomiar po terenie** i **linia wzroku** między dwoma punktami z miejscem przeszkody na profilu.
- **Panorama** z dowolnego miejsca (kamera na wysokości oczu, etykiety z odległościami).

### Interfejs
**Jakość renderowania** (zakładka „Mapa i światło”): presety od minimalnej do ultra oraz rozdzielczość renderu 25–100%, zasięg szczegółów, limit poziomu LiDAR (3/6/12/25 m), gęstość siatki kafli, rozdzielczość zdjęć lotniczych, gęstość etykiet, automatyczne obniżanie rozdzielczości i limit 30 kl./s – z licznikiem wydajności na żywo.

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

| `tiles/{z}/{x}/{y}.png` + `tiles/index.json` | piramida kafli wysokości HD (RGB: `(R·65536 + G·256 + B)/10 − 1000` m) | **NMT GUGiK** (LiDAR, geoportal.gov.pl), **DMR 5G © ČÚZK**, Terrarium |

Ortofoto (ładowane na żądanie z usług WMS/ArcGIS): **© GUGiK** (geoportal.gov.pl), **© ÚGKK SR** (ZBGIS), **© ČÚZK**.

Odświeżenie danych (np. po zmianach w OSM) albo dodanie regionu w `regions.json`:

```bash
npm run bake:all        # OSM + LiDAR dla wszystkich regionów
npm run bake -- tatry   # etap 1: teren globalny, szlaki, punkty, pokrycie terenu
npm run bake:hd -- tatry  # etap 2: LiDAR → piramida kafli, model analityczny i wysokości szlaków
```

Etap 2 pobiera setki MB danych LiDAR (GUGiK udostępnia NMT w układzie PUWG‑1992 – przeliczenie do Web Mercatora jest w `scripts/lib/puwg.mjs`). Słowacki DMR 5.0 nie ma publicznej usługi z surowymi wysokościami (tylko wysyłka na dysku), dlatego słowacka strona gór korzysta z modelu globalnego.

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
