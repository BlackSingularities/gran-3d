# Plan: przebudowa pobierania, wypiekania, UI i 3D (GRAŃ 2.x)

Spisane 2026-11-03 na podstawie przeglądu kodu (`rust-native` @ e674544) i działającej
instalacji użytkownika. Dokument dla kolejnej sesji – zawiera diagnozę, decyzje i kroki.

## Zasady repo (przypomnienie)
- Praca na gałęzi → PR do `main` → merge. **Nie budujemy lokalnie** – kompilacja/testy tylko
  w GitHub Actions (`.github/workflows/build.yml`), błędy poprawiamy z logów CI.
- Wydanie = podbicie `version` w `Cargo.toml` + `Cargo.lock` + `build/release-notes/<wersja>.md`.
- W UI/CSS **nigdy pionowej belki po lewej** (`border-left` i zamienniki) – wyróżnienia tłem,
  obwódką, kropką, górną linią. W egui: nie rysować paska przy lewej krawędzi kart.
- Wszystkie teksty po polsku, z pełnymi znakami diakrytycznymi.

## Prośba użytkownika (streszczenie)
1. Wyrzucić i przerobić cały system pobierania: na starcie widać obszary **już pobrane w
   bibliotece**; można zaznaczyć nowe do pobrania; **nigdy nie pobierać 2× tego samego**
   (nakładające się obszary, wyższy poziom szczegółów na obszarze już pobranym).
2. „Pobieranie chyba się nie odbywa” – zbadać na działającej aplikacji.
3. Interfejs „paskudny”, brak ikon – przerobić.
4. Pobieranie i wypiekanie szybsze, zoptymalizowane, **wielowątkowe** (cały komputer do
   dyspozycji).
5. Aplikacja ma być „naprawdę mocarna” w pokazywaniu 3D.

## Diagnoza stanu obecnego

### Co zastałem na dysku użytkownika
- Ustawienia `%APPDATA%\GRAN\gran.json`: `dataDir = H:\GRAN-dane`, limit 500 GB.
- `H:\GRAN-dane\areas.json`: 4 obszary. Trzy „Szczegółowe” (`complete: true`, Orawa/Tatry,
  mocno się nakładają) i czwarty `Lidar` (19.45–20.53 E, 48.98–49.33 N) z `tiles_done: 0,
  complete: false` – **pokrywa prawie w całości obszar 3**.
- Proces `Gran.exe` (E:\Program Files\GRAŃ) działa; paczki w `tiles\sat\14`, `tiles\ortho\15`,
  `tiles\dem\14..15` są dopisywane na bieżąco (ostatni zapis 17:56:05 przy sprawdzeniu 17:56:51).
  **Pobieranie więc idzie**, tylko: (a) wolno – ortofoto/LiDAR to pojedyncze zapytania WMS/WCS
  z limitem 90 s; (b) w UI widać tylko pasek jednego bieżącego zadania w menedżerze map (D),
  w atlasie 3D jedynie napis „Mapy ⟳”. Użytkownik nie ma jak zobaczyć, że coś się dzieje.
- Nazwy w `areas.json` są w UTF-8 – „krzaki” w PowerShellu 5.1 to tylko sposób wyświetlania.

### Problemy w kodzie
- `src/app.rs` `pump_queue`: kolejka **ściśle szeregowa** (`current: Option<(Task, Arc<Job>)>`),
  jedno zadanie naraz po 8 wątków (`download::WORKERS`). Obszar LiDAR blokuje wszystko za nim.
- `Task::Area` → `Job::download(a.tiles().collect())`: lista kafli liczona **na wątku UI**
  (dla poziomu Lidar dodatkowo `national::has_ortho/has_lidar` na każdym kaflu).
- Deduplikacja jest tylko „przypadkowa”: `download_tile` pomija kafel, gdy `store.has()`.
  Ale: postęp/szacunek (`areas::estimate`, okno „Nowy obszar”) liczy cały prostokąt, więc
  użytkownik widzi np. „~2 GB”, choć 90% już jest; `tiles_done`/ETA są mylące; `Job.done`
  wlicza pominięte; brak rozróżnienia „do pobrania” vs „już w bibliotece”.
- Poziomy się nakładają: Lidar ⊃ Detailed ⊃ Standard ⊃ Preview (`Level::max_z`). Obszar
  Lidar na terenie obszaru Detailed powinien pobrać tylko ortho z13–15 i LiDAR z14–15
  (+ ewentualnie brakujące krawędzie), a dziś iteruje znowu wszystko i odpytuje dysk per kafel.
- **Błąd współbieżności w `src/pack.rs`**: `Packs::get` przy `map.len() >= MAX_OPEN (192)`
  robi `map.clear()`. Inny wątek trzyma wtedy stary `Arc<Open>` z własnym `write: Mutex`,
  a następne `get` otwiera ten sam plik drugi raz z **nowym** mutexem → dwa wątki mogą
  jednocześnie liczyć `offset = metadata().len()` i nadpisać sobie dane/indeks. Przy 8+
  wątkach i dużym obszarze (setki paczek) to realne uszkodzenie paczek. Naprawić najpierw.
- `Packs::state()` czyta wpis indeksu z dysku przy każdym zapytaniu (16 B `seek_read`),
  a `get(create=false)` otwiera plik – przy skanowaniu dziesiątek tysięcy kafli to drogie.
- `Ortho` w `download::fetch_any` dla każdego kafla dekoduje JPEG Sentinel-2 przodka
  (`get(store, Layer::Sat, a)` + `load_from_memory` + crop) – ten sam przodek z14 jest
  dekodowany dla 4 kafli z15 itd. Warto cache'ować zdekodowane tło (LRU po `TileId`).
- Wypiekanie (`src/bake.rs`) samo w sobie jest tanie (Terrarium PNG → u16 delta + LZ4);
  koszt to dekodowanie PNG/TIFF i sieć. Wielowątkowość ma być na poziomie puli: sieć ≠ CPU.
- `Job::run` tworzy surowe `thread::spawn` na każde zadanie; `rayon` jest w zależnościach,
  ale nie dla pobierania.

## Docelowa architektura

### 1. Biblioteka (zamiast „listy obszarów do pobrania”)
Nowy moduł `src/library.rs` (zastępuje logikę kolejki z `app.rs`; `areas.rs` zostaje jako
model danych + migracja `areas.json`):
- **Pokrycie**: struktura trzymająca, które kafle są na dysku – bitmapa na paczkę
  (256 bitów „Data”, 256 bitów „Missing”) dla każdej `(warstwa, z, bx, by)`, budowana raz
  przy starcie w tle z indeksów paczek (`pack::list` + odczyt 4 KB nagłówka; rayon po plikach)
  i aktualizowana przy każdym zapisie. `Store::has` czyta z pamięci zamiast z dysku.
- **Plan pobrania** `plan(bbox, level) -> Plan { per_layer_z: Vec<(Layer, z, Vec<TileId>)>,
  tiles_new, bytes_new, tiles_have, bytes_have }` liczony w tle (rayon) jako **różnica**
  kafli prostokąta i pokrycia. Szacunek w UI pokazuje „do pobrania ~X, już w bibliotece Y%”.
- **Scalanie obszarów**: obszar = (prostokąt, poziom). Przy dodaniu nowego, który zawiera
  się w istniejącym o poziomie ≥ – komunikat „już masz to w bibliotece” i brak zadania.
  Nachodzące obszary zostają osobnymi wpisami (nazwy użytkownika), ale plan pobrania zawsze
  odejmuje pokrycie, więc nic nie leci 2×. Opcjonalnie: „podnieś poziom” istniejącego obszaru
  (zmienia `level`, plan liczy tylko brakujące warstwy/poziomy).
- Stan obszaru wyliczany z pokrycia (procent kafli obecnych), a nie z `tiles_done`/`complete`
  zapisanych po zadaniu. `complete` zostaje w JSON tylko dla zgodności.

### 2. Silnik pobierania (`src/download.rs` – przepisać)
- **Jedna globalna kolejka kafli** z priorytetami zamiast kolejki zadań:
  priorytet 0 = kafle widoczne w atlasie (na żądanie), 1 = model globalny, 2 = obszary
  w kolejności dodania. Wszystkie obszary pobierają się równolegle w tej samej puli,
  deduplikacja w kolejce (`HashSet<(Layer, TileId)>` „w locie”), więc dwa obszary ani widok
  3D nie pobiorą tego samego kafla naraz.
- **Osobne pule wątków na źródło** (każde ma inne limity serwera):
  - AWS Terrarium (`s3.amazonaws.com`): 32 równoległe połączenia (S3 to wytrzyma).
  - EOX Sentinel-2: 16.
  - GUGiK ORTO/NMT, ZBGIS, ČÚZK: 4–6 każde (WMS/WCS wolne, nie zostać zablokowanym).
  Pule sieciowe tylko pobierają bajty; **dekodowanie i wypiekanie** idzie do puli CPU
  (`rayon`, `available_parallelism()`), zapis do paczek przez kanał do wątku(ów) zapisu
  pogrupowanych per paczka (mniej blokad, sekwencyjne dopisywanie).
- ureq: `max_idle_connections_per_host` dopasowane do pul, keep-alive; rozważyć HTTP/2
  (ureq 3 / reqwest) tylko jeśli CI się zgodzi – nie jest konieczne.
- Ponawianie z wykładniczą przerwą, rozróżnienie 404/400 (Missing) od 429/5xx (spowolnij
  daną pulę – prosty limiter AIMD per host).
- Postęp per obszar i globalny: kafle nowe/pobrane, MB/s, kafle/s, ETA liczona tylko
  z kafli faktycznie pobieranych; ostatni błąd per źródło.
- Wznawianie: po starcie plan liczony z pokrycia, więc przerwane obszary same się dokańczają.
- Pauza/wznowienie i anulowanie per obszar oraz globalnie.
- Ortofoto: cache LRU zdekodowanych teł Sentinel-2 (np. 256 obrazów), żeby nie dekodować
  tego samego przodka wielokrotnie.

### 3. Paczki (`src/pack.rs`)
- Naprawić błąd `MAX_OPEN` (patrz diagnoza): zamiast `clear()` – LRU, które nie zamyka
  paczki trzymanej przez kogoś (`Arc::strong_count > 1`), albo mutex zapisu trzymany
  w osobnej mapie `PackKey -> Arc<Mutex<()>>` niezależnej od cache plików. Podnieść limit
  (np. 1024 uchwytów – Windows to udźwignie).
- Indeks paczki trzymany w pamięci po otwarciu (4 KB) – `state()` bez I/O.
- Zapis wsadowy: wiele kafli tej samej paczki jednym `write_at` danych + jednym zapisem
  indeksu.
- Test jednostkowy na współbieżny zapis wielu wątków do tej samej paczki przy małym
  `MAX_OPEN` (musi przejść w CI).

### 4. UI (egui, `src/ui/theme.rs`, `src/app.rs`, `src/ui/wizard.rs`, `src/ui/panels.rs`)
- **Ikony**: dodać font ikon (np. Phosphor przez crate `egui-phosphor` w wersji zgodnej
  z egui 0.33 – sprawdzić na crates.io; alternatywa: wbudować TTF Lucide/Phosphor przez
  `include_bytes!` i `FontDefinitions`). Ikony na przyciskach paska, w kartach obszarów
  (pobrane ✓, w toku, pauza, błąd), w narzędziach atlasu.
- **Menedżer map (D) jako „Biblioteka”**:
  - Mapa 2D: pobrane kafle zaznaczone półprzezroczystą warstwą pokrycia (kolor wg poziomu),
    obszary z obwódką; zaznaczanie nowego prostokąta pokazuje na żywo, co jest nowe
    (kreskowanie) a co już jest.
  - Panel po prawej: zajętość dysku, lista obszarów z miniaturą/ikoną, procentem pokrycia,
    przyciskami (pokaż w 3D, podnieś poziom, pauza, usuń); sekcja „Pobieranie” z globalną
    prędkością (MB/s, kafle/s), ETA i stanem źródeł (AWS/EOX/GUGiK/ZBGIS/ČÚZK).
  - Okno „Nowy obszar”: poziomy jako karty z ikoną i szacunkiem **tylko brakujących danych**.
- **Atlas 3D**: stały, mały wskaźnik pobierania na górnym pasku (ikona + % + MB/s),
  kliknięcie otwiera bibliotekę. Toasty przy zakończeniu obszaru.
- Przy starcie: jeśli biblioteka ma obszary – od razu atlas z nimi; jeśli pusta – kreator.
- Pamiętać: zero pionowych pasków przy lewej krawędzi kart (zasada użytkownika).

### 5. Render 3D (`src/render/*`, `src/ui/atlas.rs`)
Do zbadania i ulepszenia w osobnym PR (po pobieraniu):
- Wczytywanie kafli do GPU w tle wielowątkowo (dekodowanie DEM/JPEG w rayon, upload
  wsadowy), większy budżet kafli w pamięci GPU dla mocnych kart (ustawienie „jakość”).
- LOD: geomorfing/skirts przeciw szczelinom, wyższa gęstość siatki blisko kamery,
  MSAA 4×, anizotropia tekstur, mipmapy ortofoto.
- Oświetlenie: cienie terenu od słońca (`sun.rs` już jest) – shadow map lub horizon
  mapping, ambient occlusion z DEM, atmosfera/mgła z odległością, niebo (`sky.wgsl`).
- Wydajność: pomiar FPS i czasu klatki w panelu debug, `PresentMode::Mailbox`/`AutoNoVsync`
  jako opcja.
- Sprawdzić zgłoszone wcześniej artefakty: „niebieskie rowy z szarymi ścianami” to kafle
  terenu z importu 1.x (`Job::import_legacy`) – prawdopodobnie złe wysokości/NoData
  w części kafli; zweryfikować dekodowanie i ewentualnie odrzucać kafle z nierealnymi
  skokami lub je przepobrać.

## Kolejność prac (każdy punkt = osobny PR do `main`, CI musi być zielone)
1. `pack.rs`: naprawa wyścigu `MAX_OPEN`, indeks w pamięci, test współbieżności.
2. `library.rs`: pokrycie z indeksów paczek, `plan()` jako różnica, testy (plan obszaru
   zawartego w już pobranym = 0 kafli; obszar Lidar na Detailed = tylko ortho+LiDAR).
3. Nowy `download.rs`: globalna kolejka z priorytetami, pule per źródło, pula CPU, zapis
   wsadowy, postęp/ETA/MB/s, pauza. Podpiąć w `app.rs` w miejsce `Task`/`queue`/`current`;
   przenieść pobieranie na żądanie z `render/tiles.rs` i `ui/map.rs` do tej samej kolejki
   (priorytet 0) – sprawdzić, jak dziś dociągają kafle (`download::get`).
4. UI: ikony, Biblioteka, wskaźnik w atlasie, okno nowego obszaru z różnicą.
5. Render 3D (osobny PR lub kilka).
6. Wydanie: podbić wersję (np. 2.1.0) w `Cargo.toml`/`Cargo.lock`, `build/release-notes/2.1.0.md`.

## Pliki do przeczytania na start
`src/app.rs` (kolejka, menedżer, kreator), `src/download.rs`, `src/pack.rs`, `src/store.rs`,
`src/areas.rs`, `src/bake.rs`, `src/national.rs` (ortho/LiDAR), `src/render/tiles.rs`
(kafle na żądanie w 3D), `src/ui/map.rs`, `src/ui/wizard.rs`, `src/ui/theme.rs`.
