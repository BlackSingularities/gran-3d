# GRAŃ — atlas szlaków górskich 3D

Natywny program dla Windows (Rust, wgpu, egui): trójwymiarowy atlas gór łączący rzeczywisty model terenu z siecią znakowanych szlaków z OpenStreetMap, z narzędziami analitycznymi — wysokością względną, wyznaczaniem tras z wariantami, polem widoczności, linią wzroku, cieniami o dowolnej porze dnia i przelotem kamery nad trasą.

Jeden plik `Gran.exe`, bez przeglądarki, Node i lokalnego serwera. Dystrybucja przez [redzikowski.com](https://redzikowski.com/portfolio/gran-3d) (pliki z GitHub Releases tego repozytorium). Repozytorium nie zawiera danych gór: teren, zdjęcia, szlaki i punkty pobierane są w miarę oglądania mapy, a w menedżerze map (przycisk „Mapy” albo `D`) można zapisać obszary do pracy bez internetu.

## Instalacja

| platforma | plik | uwagi |
| --- | --- | --- |
| Windows 10/11 (64-bit) | `GRAN-Setup-<wersja>.exe` | instalator dla wszystkich użytkowników (`Program Files`, okno UAC, wybór folderu), skróty, deinstalacja z Panelu sterowania, aktualizacje z redzikowski.com |

Instalacja na wersji 1.x (Electron) usuwa ją jej własnym deinstalatorem i zachowuje dane. Przy pierwszym uruchomieniu kreator pyta o **folder na dane map** (podfolder `GRAN-dane`), **maksymalny rozmiar** (1–500 GB) i obszary do pobrania na start. Kafle terenu wersji 1.x (także LiDAR) są importowane w tle.

## Funkcje

### Teren
- Teren kaflowy z poziomami szczegółów (drzewo czwórkowe Web Mercator, siatka 64×64 na kafel, fartuchy maskujące szczeliny, krzywizna Ziemi), dane wczytywane w wątkach wokół kamery, a do czasu wczytania – fragmenty kafli nadrzędnych.
- **Ortofotomapa** GUGiK (PL), ČÚZK (CZ) i ZBGIS (SK) do ~0,6 m na bliskich kaflach w stylu satelitarnym, a w obszarach „LiDAR i ortofoto” – **model LiDAR 3 m** (NMT GUGiK w PUWG-1992 przeliczany do Web Mercatora, DMR 5G ČÚZK).
- **Pokrycie terenu z OSM** (wody, lasy, kosodrzewina, piargi) wypiekane do masek sektorów i używane w cieniowaniu.
- Style: **teren** (piętra roślinności zależne od wysokości, nachylenia i ekspozycji, śnieg według daty, barwy jesieni), **satelita** (Sentinel-2 cloudless), **mapa**, **hipsometria**, **nachylenie jak na mapach lawinowych**, **ekspozycja**.
- **Słońce według daty i godziny** (NOAA, czas polski), **cienie rzucane przez rzeźbę** liczone w tle, perspektywa powietrzna i niebo z poświatą Słońca.
- Poziomice z automatycznym cięciem, siatka kilometrowa, przewyższenie ×1–3, MSAA i skalowanie rozdzielczości renderu.

### Trasy
- Graf szlaków z relacji `route=hiking|foot` OSM (kolory PTTK z `osmc:symbol`) w sektorach ~50×50 km łączonych bez szwów; wysokości z modelu terenu co ~10 m.
- **Warianty naraz**: najszybsza, najkrótsza (po powierzchni), najmniej podejść, najłatwiejsza (unika T4–T6) i objazdy metodą kar; zbieżne warianty łączone.
- Dowolna liczba punktów pośrednich, przeciąganie punktów, menu pod prawym przyciskiem.
- **Trasa przez teren**: A* po modelu (16 kierunków), funkcja Toblera, limit nachylenia.
- Metryki: dystans w rzucie i po powierzchni, linia prosta i krętość, podejścia/zejścia z histerezą, czas wg **reguły PTTK** lub **DIN 33466** z tempem i skalą SAC, **punkty GOT PTTK**, maks. nachylenie, ocena trudności.
- **Profil wysokości** barwiony nachyleniem, z pasem kolorów szlaków, mijanymi punktami, wariantami w tle i kursorem zsynchronizowanym z mapą.
- **Przelot kamery** nad trasą, eksport i import **GPX** (także przeciągnięciem pliku na okno), **link `gran://`** z widokiem i punktami trasy (rejestrowany przez instalator, otwierany też z wyszukiwarki).

### Analizy
- **Soczewki** (`H` `W` `O`): ta sama wysokość (pas ± lub poziomica), wysokość względna, widoczność z miejsca pod kursorem; kliknięcie przypina, `Esc` odpina.
- **Pole widoczności** (32 km, krzywizna Ziemi i refrakcja, oko 1,7/10/30/100 m) z powierzchnią i listą widocznych szczytów.
- **Pomiar po terenie** i **linia wzroku** między dwoma punktami.
- **Panorama** z dowolnego miejsca z odległościami do szczytów.

### Interfejs
Wyszukiwarka (`/`) szczytów, schronisk, przełęczy, szlaków (także po kolorze) i miejsc w Europie (Nominatim), panel informacji, karta „Mapa i światło”, presety jakości z licznikiem klatek, skróty klawiszowe (`?`), kompas, podziałka, odczyt kursora (współrzędne, wysokość, nachylenie, ekspozycja), zrzut ekranu (`S`).

## Dane na dysku

```text
<folder>\GRAN-dane\
  areas.json                    obszary zapisane do pracy bez internetu
  tiles\dem\z\bx_by.gpk         teren (u16 co 1/8 m + LZ4), paczki 16×16 kafli
  tiles\sat\z\bx_by.gpk         zdjęcia satelitarne (JPEG), paczki 16×16 kafli
  tiles\ortho\zx_by.gpk       ortofotomapa krajowa (JPEG 512 px)
  sectors\9_x_y.json.lz4        szlaki i punkty OSM sektora
  land\9_x_y.png                pokrycie terenu sektora (R woda, G las, B skały/zarośla)
```

Po przekroczeniu limitu najdawniej używane paczki są usuwane; zapisane obszary i model globalny (do z6) zostają. Ustawienia: `%APPDATA%\GRAN\gran.json` (i `datadir.txt` dla deinstalatora).

Źródła: teren **Terrarium** (AWS Open Data: SRTM, EU-DEM), LiDAR **NMT GUGiK** i **DMR 5G ČÚZK** (import z wersji 1.x), zdjęcia **Sentinel-2 cloudless 2020 © EOX** (CC BY-NC-SA 4.0), szlaki i punkty **© OpenStreetMap** (ODbL) przez Overpass API.

## Budowanie

```bash
cargo run              # tryb deweloperski
cargo test
cargo build --release  # target/release/Gran.exe
```

Instalator: `makensis /DVERSION=<wersja> build\gran.nsi` (NSIS 3). Wydanie: podbij `version` w `Cargo.toml`, dodaj `build/release-notes/<wersja>.md` i scal zmiany do `main` – workflow `.github/workflows/build.yml` uruchomi testy, zbuduje i podpisze program oraz instalator, utworzy tag `v<wersja>` i opublikuje wydanie z `latest.yml`. Podpis cyfrowy (sekrety): `WIN_CSC_LINK` (certyfikat .pfx w base64) i `WIN_CSC_KEY_PASSWORD`.

Zmienne do testów: `GRAN_WIZARD_PAGE=folder|limit|areas`, `GRAN_CAPTURE=plik.png` i `GRAN_CAPTURE_AFTER=sekundy`; `APPDATA` / `LOCALAPPDATA` w katalogu tymczasowym dają czyste ustawienia.

---

**Projekt i wykonanie: [redzikowski.com](https://redzikowski.com)**

<a href="https://buycoffee.to/adamredzikowski" target="_blank"><img src="https://buycoffee.to/static/img/share/share-button-dark--pl.png" style="width: 351px; height: 92px" width="351" height="92" alt="Postaw kawę dla adamredzikowski na buycoffee.to"></a>
