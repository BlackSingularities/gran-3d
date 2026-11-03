; GRAŃ – dodatki do instalatora NSIS (electron-builder)
; Instalacja dla wszystkich użytkowników (Program Files) wymaga uprawnień administratora – okno UAC pojawia się przy starcie.

!macro customUnInstall
  ; przy aktualizacji (stary deinstalator uruchamiany w tle) dane użytkownika zostają nietknięte
  ${ifNot} ${isUpdated}
    IfSilent gran_skip_data
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Usunąć także pobrane dane map (kafle terenu, zapisane obszary) i ustawienia programu?$\r$\n$\r$\nWybierz „Nie”, aby zachować je na wypadek ponownej instalacji." /SD IDNO IDNO gran_skip_data
      SetShellVarContext current
      ; folder danych wybrany przez użytkownika (zapisany przez program w datadir.txt, UTF-16 LE)
      ClearErrors
      FileOpen $0 "$APPDATA\GRAN\datadir.txt" r
      ${ifNot} ${Errors}
        FileReadUTF16LE $0 $1
        FileClose $0
        ; usuwany jest tylko własny podfolder programu – nigdy wskazany katalog nadrzędny
        StrCpy $2 $1 9 -9
        ${if} $2 == "GRAN-dane"
          RMDir /r "$1"
        ${endIf}
      ${endIf}
      RMDir /r "$LOCALAPPDATA\GRAN"
      RMDir /r "$APPDATA\GRAN"
      SetShellVarContext all
    gran_skip_data:
  ${endIf}
!macroend
