#!/bin/sh
# Zrzuty ekranu GRANI w emulatorze (workflow Screenshots). Każdy widok otwiera link gran:// przekazany
# w intencji; `gran_capture` pomija przewodnik i pytania. Logcat zbierany zawsze (diagnostyka).
PKG=com.blacksingularities.gran3d
shot() {
  name="$1"; link="$2"; wait="$3"
  # cudzysłowy dla zdalnej powłoki: średniki w linku nie mogą rozdzielać poleceń
  adb shell "am start -n $PKG/.GranActivity --ez gran_capture true --es gran_link '$link'"
  sleep "$wait"
  if adb shell pidof "$PKG" >/dev/null; then echo "$name: aplikacja działa"; else echo "::warning::$name: aplikacja nie działa"; fi
  adb exec-out screencap -p > "shots/$name.png"
}
shot 01-teren 'gran://widok?lat=49.1930&lon=20.0720&az=200&nach=38&odl=5200&styl=4' 150
shot 02-trasa 'gran://widok?lat=49.2320&lon=19.9820&az=160&nach=45&odl=7000&styl=0&trasa=49.2700,19.9800;49.2318,19.9817;49.2210,20.0090' 120
shot 03-nachylenie 'gran://widok?lat=49.1790&lon=20.0880&az=320&nach=52&odl=4200&styl=2' 90
adb logcat -d > shots/logcat.txt
adb shell run-as "$PKG" cat files/../../../../storage/emulated/0/Android/data/$PKG/files/GRAN/gran.log > shots/gran.log 2>/dev/null || true
adb shell cat "/storage/emulated/0/Android/data/$PKG/files/GRAN/gran.log" >> shots/gran.log 2>/dev/null || true
exit 0
