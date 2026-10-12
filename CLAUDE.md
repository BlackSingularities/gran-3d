# GRAŃ – zasady pracy w repozytorium

- **Każda zmiana trafia do `main`**: praca na gałęzi roboczej → pull request do `main` → scalenie (merge). Nie zostawiamy zmian tylko na gałęzi.
- **Wydania** (Windows `.exe` + Android `.apk` w GitHub Releases) robi workflow `.github/workflows/build.yml`:
  push na `main` z nową wersją w `package.json` (brak tagu `v<wersja>`) → build obu platform, tag i wydanie.
  Bez podbicia wersji push na `main` to tylko kontrolna kompilacja.
- Przy wydaniu podbij wersję w `package.json` **i** `mobile/package.json` (oraz lockfile: `npm install --package-lock-only` w obu katalogach) i dodaj opis `build/release-notes/<wersja>.md`.
- Aplikacje same proponują aktualizację: Windows przez `electron-updater` (`latest.yml`), Android przez baner (`/api/update` → GitHub Releases). APK podpisuje stały klucz z sekretów `ANDROID_KEYSTORE_*` – nie zmieniać, inaczej aktualizacje na telefonach przestaną się instalować.
- Interfejs: na telefonach (≤ 760 px) układ w stylu Material 3 (`src/style.css`, sekcja „Telefony”), na urządzeniach dotykowych bez skrótów klawiszowych; celownik na środku ekranu działa jak kursor (`App.trackAim`, `Chrome.updateAim`).
- Kontrole przed pushem: `npm run build` (typy + Vite); serwer/skrypty muszą działać na Node 18 (silnik w aplikacji Android).
