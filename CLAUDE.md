# GRAŃ – zasady pracy w repozytorium

- **Każda zmiana trafia do `main`**: praca na gałęzi roboczej → pull request do `main` → scalenie (merge). Nie zostawiamy zmian tylko na gałęzi.
- **Tylko Windows, tylko Rust.** Program natywny (Rust + wgpu + egui) w katalogu głównym (`Cargo.toml`, `src/`). Wersji webowej/Electron i Androida już nie ma (usunięte w 2.0.0).
- **Nie budujemy i nie testujemy lokalnie** – kompilację i testy robi GitHub Actions; błędy poprawiamy na podstawie logów przebiegu.
- **Wydania** (instalator NSIS `GRAN-Setup-<wersja>.exe` + `latest.yml` w GitHub Releases) robi workflow `.github/workflows/build.yml`:
  push na `main` z nową wersją w `Cargo.toml` (brak tagu `v<wersja>`) → testy, build, podpis, tag i wydanie.
  Bez podbicia wersji push na `main` to tylko kontrolna kompilacja.
- Przy wydaniu podbij `version` w `Cargo.toml` (i `Cargo.lock`) i dodaj opis `build/release-notes/<wersja>.md`.
- **Pobieranie i aktualizacje idą przez redzikowski.com** (projekt `gran-3d` w portfolio strony, `releaseSource: 'github'`). Program pyta `redzikowski.com/api/github/repos/BlackSingularities/gran-3d/releases/latest`, pobiera instalator przez serwer strony, sprawdza SHA-512 z `latest.yml` i uruchamia go z UAC (`src/updater.rs`). Wydanie bez `latest.yml` zablokuje aktualizacje.
- Instalator NSIS (`build/gran.nsi`, UTF-8 z BOM) instaluje dla wszystkich użytkowników (Program Files, administrator), usuwa wersję 1.x jej deinstalatorem; tryb cichy `/S`, `--force-run` uruchamia program po instalacji.
- Dane map: jeden folder wybrany przez użytkownika (`<wybrany>\GRAN-dane`) z limitem 1–500 GB: paczki kafli `tiles\`, sektory OSM `sectors\`, `areas.json`. Ustawienia w `%APPDATA%\GRAN\gran.json`, ścieżka danych też w `datadir.txt` (dla deinstalatora).
- Struktura: `render/` (kamera, kafle, shadery WGSL), `ui/atlas.rs` (widok 3D, narzędzia), `ui/panels.rs`, `graph.rs` (trasy), `osm.rs` + `world.rs` (sektory OSM), `heights.rs`, `analysis.rs`, `store.rs` + `pack.rs` (dane na dysku).
