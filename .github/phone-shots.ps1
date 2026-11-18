# Zrzuty układu telefonu (412×915 pt przy skali 2) z programu na Windows – ten sam interfejs co na
# Androidzie (wąski ekran: dolny pasek narzędzi, arkusz szczegółów). Każdy widok otwiera link gran://.
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force shots | Out-Null
$shots = @(
  @('01-teren', 'gran://widok?lat=49.2040&lon=20.0610&az=200&nach=36&odl=5600&styl=4', 120),
  @('02-trasa', 'gran://widok?lat=49.2400&lon=19.9850&az=165&nach=48&odl=7600&styl=0&trasa=49.2700,19.9800;49.2318,19.9817;49.2210,20.0090', 120),
  @('03-warunki', 'gran://widok?lat=49.1795&lon=20.0880&az=320&nach=40&odl=5200&styl=4&panel=warunki', 100),
  @('04-punkty', 'gran://widok?lat=49.2320&lon=19.9820&az=180&nach=42&odl=6000&styl=0&panel=punkty', 90),
  @('05-offline', 'gran://widok?lat=49.2000&lon=20.0300&az=0&nach=60&odl=26000&styl=4&panel=offline', 90),
  @('06-nachylenie', 'gran://widok?lat=49.1790&lon=20.0880&az=320&nach=52&odl=4200&styl=2', 90),
  @('07-powitanie', 'gran://widok?lat=49.2040&lon=20.0610&az=200&nach=36&odl=5600&styl=4', 90)
)
foreach ($s in $shots) {
  $env:GRAN_CAPTURE = (Join-Path (Resolve-Path shots) ($s[0] + '.png'))
  $env:GRAN_CAPTURE_AFTER = [string]$s[2]
  $env:GRAN_WINDOW = '360x760'
  $env:GRAN_ZOOM = '1'
  $env:WGPU_BACKEND = 'dx12'
  $env:GRAN_LANG = 'pl'
  if ($s[0] -eq '07-powitanie') { $env:GRAN_TOUR = '0' } else { Remove-Item Env:GRAN_TOUR -ErrorAction SilentlyContinue }
  $p = Start-Process $env:GRAN_EXE -ArgumentList $s[1] -PassThru
  if (-not $p.WaitForExit(($s[2] + 90) * 1000)) { Stop-Process $p -Force; Write-Host "::warning::$($s[0]): przekroczony czas" }
  Write-Host "$($s[0]): $(Test-Path $env:GRAN_CAPTURE)"
  # awaria w trakcie zrzutu: raport do artefaktu, a kolejny widok bez okna zgłoszenia
  $crash = "$env:LOCALAPPDATA\GRAN\feedback-pending-crash.json"
  if (Test-Path $crash) { Write-Host "::warning::$($s[0]): awaria"; Copy-Item $crash "shots\$($s[0])-awaria.json"; Remove-Item $crash }
  Copy-Item "$env:LOCALAPPDATA\GRAN\gran.log" "shots\$($s[0]).log" -ErrorAction SilentlyContinue
}
Copy-Item "$env:LOCALAPPDATA\GRAN\gran.log" shots\gran.log -ErrorAction SilentlyContinue
