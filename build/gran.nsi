; GRAŃ – instalator Windows (NSIS 3, Unicode). Plik zapisany jako UTF-8 z BOM.
; makensis /DVERSION=2.0.0 /DEXE=..\target\release\Gran.exe /DOUT=..\release\GRAN-Setup-2.0.0.exe build\gran.nsi
;
; Instalacja dla wszystkich użytkowników (Program Files, okno UAC). Tryb cichy: /S; po instalacji
; w trybie cichym program uruchamia się z opcją --force-run (tak wywołuje go aktualizator).
; Instalacja na wersji 1.x (Electron) usuwa ją jej własnym deinstalatorem, zostawiając dane map.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel admin

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef EXE
  !define EXE "..\target\release\Gran.exe"
!endif
!ifndef OUT
  !define OUT "..\release\GRAN-Setup-${VERSION}.exe"
!endif

!define NAME "GRAŃ"
!define UNKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\GRAN"
; klucz deinstalacji wersji 1.x (electron-builder: UUID v5 z appId)
!define OLDKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\dbc2f3f8-5943-58c8-8a92-70537188d38c"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

Name "${NAME}"
OutFile "${OUT}"
InstallDir "$PROGRAMFILES64\${NAME}"
InstallDirRegKey HKLM "${UNKEY}" "InstallLocation"
BrandingText "GRAŃ ${VERSION} · redzikowski.com"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1045 "ProductName" "${NAME}"
VIAddVersionKey /LANG=1045 "FileDescription" "GRAŃ – atlas szlaków górskich 3D (instalator)"
VIAddVersionKey /LANG=1045 "CompanyName" "BlackSingularities"
VIAddVersionKey /LANG=1045 "LegalCopyright" "© 2026 Adam Rędzikowski"
VIAddVersionKey /LANG=1045 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1045 "ProductVersion" "${VERSION}"

!define MUI_ICON "icon.ico"
!define MUI_UNICON "icon.ico"
!define MUI_WELCOMEFINISHPAGE_BITMAP "installerSidebar.bmp"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "uninstallerSidebar.bmp"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Uruchom GRAŃ"
!define MUI_FINISHPAGE_RUN_FUNCTION RunApp

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Polish"

; uruchomienie bez uprawnień administratora (przez Eksploratora, jako zwykły użytkownik)
Function RunApp
  Exec '"$WINDIR\explorer.exe" "$INSTDIR\Gran.exe"'
FunctionEnd

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "GRAŃ wymaga 64-bitowego systemu Windows 10 lub 11." /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
  ; dotychczasowy folder (ta wersja albo 1.x)
  ReadRegStr $0 HKLM "${UNKEY}" "InstallLocation"
  ${If} $0 == ""
    ReadRegStr $0 HKLM "${OLDKEY}" "InstallLocation"
  ${EndIf}
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
FunctionEnd

; program musi być zamknięty, zanim podmienimy plik
!macro CloseApp
  StrCpy $1 0
  ${Do}
    ClearErrors
    Delete "$INSTDIR\Gran.exe"
    ${IfNot} ${Errors}
      ${Break}
    ${EndIf}
    IntOp $1 $1 + 1
    ${If} $1 = 12
      nsExec::Exec 'taskkill /F /IM Gran.exe'
      Sleep 800
    ${EndIf}
    ${If} $1 > 20
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Zamknij program GRAŃ, żeby kontynuować." /SD IDCANCEL IDRETRY gran_retry
      Abort
      gran_retry:
      StrCpy $1 0
    ${EndIf}
    Sleep 500
  ${Loop}
!macroend

Section "GRAŃ" SecMain
  SetShellVarContext all
  SetRegView 64
  SetOutPath "$INSTDIR"
  ${If} ${FileExists} "$INSTDIR\Gran.exe"
    !insertmacro CloseApp
  ${EndIf}

  ; wersja 1.x (Electron): jej deinstalator w trybie aktualizacji (bez pytania o dane map)
  ReadRegStr $2 HKLM "${OLDKEY}" "UninstallString"
  ${If} $2 == ""
  ${AndIf} ${FileExists} "$INSTDIR\Uninstall ${NAME}.exe"
    StrCpy $2 '"$INSTDIR\Uninstall ${NAME}.exe"'
  ${EndIf}
  ${If} $2 != ""
    DetailPrint "Usuwanie wersji 1.x…"
    ExecWait '$2 /S --updated _?=$INSTDIR'
    Delete "$INSTDIR\Uninstall ${NAME}.exe"
    RMDir /r "$INSTDIR\resources"
    RMDir /r "$INSTDIR\locales"
    DeleteRegKey HKLM "${OLDKEY}"
    SetOutPath "$INSTDIR"
  ${EndIf}

  File "/oname=Gran.exe" "${EXE}"
  WriteUninstaller "$INSTDIR\Odinstaluj.exe"

  CreateShortCut "$SMPROGRAMS\${NAME}.lnk" "$INSTDIR\Gran.exe" "" "$INSTDIR\Gran.exe" 0
  CreateShortCut "$DESKTOP\${NAME}.lnk" "$INSTDIR\Gran.exe" "" "$INSTDIR\Gran.exe" 0

  ; linki gran://widok?… otwierają widok w programie
  WriteRegStr HKCR "gran" "" "URL:GRAŃ – widok mapy"
  WriteRegStr HKCR "gran" "URL Protocol" ""
  WriteRegStr HKCR "gran\DefaultIcon" "" '"$INSTDIR\Gran.exe",0'
  WriteRegStr HKCR "gran\shell\open\command" "" '"$INSTDIR\Gran.exe" "%1"'

  WriteRegStr HKLM "${UNKEY}" "DisplayName" "GRAŃ – atlas szlaków 3D"
  WriteRegStr HKLM "${UNKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNKEY}" "DisplayIcon" "$INSTDIR\Gran.exe"
  WriteRegStr HKLM "${UNKEY}" "Publisher" "BlackSingularities"
  WriteRegStr HKLM "${UNKEY}" "URLInfoAbout" "https://redzikowski.com/portfolio/gran-3d"
  WriteRegStr HKLM "${UNKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNKEY}" "UninstallString" '"$INSTDIR\Odinstaluj.exe"'
  WriteRegStr HKLM "${UNKEY}" "QuietUninstallString" '"$INSTDIR\Odinstaluj.exe" /S'
  WriteRegDWORD HKLM "${UNKEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNKEY}" "EstimatedSize" "$0"
SectionEnd

Function .onInstSuccess
  ${If} ${Silent}
    ${GetParameters} $R0
    ClearErrors
    ${GetOptions} $R0 "--force-run" $R1
    ${IfNot} ${Errors}
      Call RunApp
    ${EndIf}
  ${EndIf}
FunctionEnd

Section "Uninstall"
  SetShellVarContext all
  SetRegView 64
  nsExec::Exec 'taskkill /F /IM Gran.exe'
  Delete "$INSTDIR\Gran.exe"
  Delete "$INSTDIR\Odinstaluj.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${NAME}.lnk"
  Delete "$DESKTOP\${NAME}.lnk"
  DeleteRegKey HKLM "${UNKEY}"
  DeleteRegKey HKCR "gran"

  ; przy aktualizacji (--updated) albo w trybie cichym dane użytkownika zostają nietknięte
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "--updated" $R1
  ${If} ${Errors}
  ${AndIfNot} ${Silent}
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Usunąć także pobrane dane map (kafle terenu, szlaki, zapisane obszary) i ustawienia programu?$\r$\n$\r$\nWybierz „Nie”, aby zachować je na wypadek ponownej instalacji." IDNO keep
      SetShellVarContext current
      ; folder danych wybrany przez użytkownika (zapisany przez program w datadir.txt, UTF-16 LE)
      ClearErrors
      FileOpen $0 "$APPDATA\GRAN\datadir.txt" r
      ${IfNot} ${Errors}
        FileReadUTF16LE $0 $1
        FileClose $0
        ; usuwany jest tylko własny podfolder programu – nigdy wskazany katalog nadrzędny
        StrCpy $2 $1 9 -9
        ${If} $2 == "GRAN-dane"
          RMDir /r "$1"
        ${EndIf}
      ${EndIf}
      RMDir /r "$LOCALAPPDATA\GRAN"
      RMDir /r "$APPDATA\GRAN"
      SetShellVarContext all
    keep:
  ${EndIf}
SectionEnd
