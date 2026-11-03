# GRAŃ – zasady pracy w repozytorium

- **Każda zmiana trafia do `main`**: praca na gałęzi roboczej → pull request do `main` → scalenie (merge). Nie zostawiamy zmian tylko na gałęzi.
- **Tylko Windows.** Wersji na Androida nie ma i nie wydajemy jej (usunięta w 1.1.0).
- **Wydania** (instalator Windows `.exe` + `latest.yml` w GitHub Releases) robi workflow `.github/workflows/build.yml`:
  push na `main` z nową wersją w `package.json` (brak tagu `v<wersja>`) → build, tag i wydanie.
  Bez podbicia wersji push na `main` to tylko kontrolna kompilacja.
- Przy wydaniu podbij wersję w `package.json` (oraz lockfile: `npm install --package-lock-only`) i dodaj opis `build/release-notes/<wersja>.md`.
- **Pobieranie i aktualizacje idą przez redzikowski.com** (projekt `gran-3d` w portfolio strony, `releaseSource: 'github'`). Program pyta `redzikowski.com/api/github/repos/BlackSingularities/gran-3d/releases/latest`, pobiera instalator przez serwer strony, sprawdza SHA-512 z `latest.yml` i uruchamia go z UAC (`electron/main.cjs`, sekcja „aktualizacje”). Wydanie bez `latest.yml` zablokuje aktualizacje.
- Instalator NSIS jest `perMachine` (Program Files, wymaga administratora); dodatki w `build/installer.nsh` (plik UTF-8 z BOM).
- Dane map: jeden folder wybrany przez użytkownika (`<wybrany>\GRAN-dane`) z limitem 1–500 GB; zawiera też bufor przeglądarki (`.cache/browser`). Ścieżka zapisana w `%APPDATA%\GRAN\settings.json` i `datadir.txt` (dla deinstalatora).
- Interfejs: w wąskich oknach (≤ 760 px) układ kompaktowy (`src/style.css`, sekcja na końcu), na ekranach dotykowych bez skrótów klawiszowych; celownik na środku ekranu działa jak kursor (`App.trackAim`, `Chrome.updateAim`).
- Kontrole przed pushem: `npm run build` (typy + Vite) i `node --check electron/main.cjs`.
