# GRAŃ 2 – wersja natywna (Rust)

Nowa, natywna wersja GRANIA dla Windows: jeden plik `Gran.exe` (Rust, wgpu, egui), bez Electrona, Node i lokalnego serwera HTTP. Docelowo zastąpi wersję 1.x z katalogu nadrzędnego, z tymi samymi funkcjami.

## Etap 1 (ta gałąź)

- **Kreator pierwszego uruchomienia**: folder na dane (podfolder `GRAN-dane`), maksymalny rozmiar (1–500 GB), obszary do pobrania na start (gotowe pasma albo prostokąt zaznaczony na mapie) i poziom szczegółów (Podgląd / Standard / Szczegółowy) z szacunkiem rozmiaru, potem pobieranie z postępem.
- **Dane wypieczone na dysku**: kafle pakowane po 16×16 do plików `.gpk` z indeksem (`src/pack.rs`). Teren zapisany jako `u16` z krokiem 1/8 m, z predykcją i kompresją LZ4 (`src/bake.rs`), zdjęcia jako JPEG. Wypiekanie dzieje się przy pobieraniu.
- **Wskazanie folderu z danymi**: program rozpoznaje dane tej wersji, a kafle terenu wersji 1.x (`.cache\dem`, także LiDAR) importuje w tle do paczek.
- **Limit rozmiaru**: sprzątacz usuwa najdawniej używane paczki; paczki zapisanych obszarów i model globalny (do z6) zostają.
- **Mapa 2D**: relief z modelu terenu albo obraz Sentinel-2, przesuwanie, przybliżanie do kursora, zaznaczanie obszarów, kafle zastępcze z poziomów wyżej.
- **Panel „Dane map”**: zajętość, obszary z postępem, dokańczanie, usuwanie; ustawienia (`Ctrl+,`) z przenoszeniem danych do nowego folderu.

## Kolejne etapy

2. Renderer terenu 3D (wgpu) z poziomami szczegółów, ortofoto GUGiK / ZBGIS / ČÚZK i LiDAR (NMT GUGiK, DMR 5G ČÚZK) wypiekany do paczek.
3. Szlaki OSM, wyszukiwarka, trasowanie z wariantami i profil.
4. Analizy: soczewki, pole widoczności, słońce i cienie.
5. Instalator (tryb administratora), aktualizacje z redzikowski.com, wpis w portfolio, wydanie 2.0.

## Budowanie

```bash
cargo run              # tryb deweloperski
cargo test
cargo build --release  # target/release/Gran.exe
```

Zmienne do testów: `GRAN_WIZARD_PAGE=folder|limit|areas` (strona kreatora), `GRAN_CAPTURE=plik.png` i `GRAN_CAPTURE_AFTER=sekundy` (zrzut okna i zamknięcie programu). `APPDATA` / `LOCALAPPDATA` ustawione na katalog tymczasowy dają czyste ustawienia.
