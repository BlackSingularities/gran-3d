## GRAŃ 1.0.0 – pierwsze wydanie aplikacji na Windows i Androida

Atlas szlaków górskich 3D jako samodzielna aplikacja – bez strony WWW i bez serwera w internecie.

### Pliki
- **`GRAN-Setup-1.0.0.exe`** – instalator Windows 10/11 (64-bit)
- **`GRAN-1.0.0.apk`** – Android 7.0 lub nowszy (ARM64, ARMv7, x86_64)

### Windows
- Instalator z wyborem folderu (domyślnie `Program Files`), skrótami na pulpicie i w menu Start oraz deinstalacją z Panelu sterowania (z opcją usunięcia pobranych danych).
- **Automatyczne aktualizacje** z tej strony wydań.
- Skróty okna: `F11` pełny ekran, `Ctrl` + `+`/`-`/`0` powiększenie interfejsu, `Ctrl+,` ustawienia danych.

### Android
- Pełny silnik danych działa na telefonie (Node.js wbudowany w aplikację) – pobieranie terenu LiDAR, szlaków i danych okolic bez zewnętrznego serwera.
- Sterowanie dotykiem: przeciąganie, dwa palce – przybliżenie i obrót, przytrzymanie – menu analiz.
- Eksport GPX i zrzuty widoku zapisywane w folderze **Pobrane/Gran**.
- Instalacja z pliku: zezwól przeglądarce lub menedżerowi plików na instalowanie nieznanych aplikacji.

### Dane map (obie platformy)
Przy pierwszym uruchomieniu wybierasz:
- **zachowywanie danych** z **limitem pojemności** (1–500 GB; po przekroczeniu najdawniej używane kafle są usuwane automatycznie), albo
- **usuwanie przy każdym zamknięciu** programu.

Zmiana w zakładce **Jakość → Dane i pamięć**, tam też zajętość i „Wyczyść dane teraz”.

### Funkcje
Interfejs i funkcje 1:1 z dotychczasową wersją: teren LiDAR 3–12 m, ortofotomapa, szlaki PTTK z OSM, trasy z wariantami i czasem PTTK/DIN, wysokość względna, pole widoczności, linia wzroku, cienie o dowolnej porze, przelot kamery, GPX.
