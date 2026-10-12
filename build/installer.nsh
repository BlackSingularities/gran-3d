; GRAŃ – dodatki do instalatora NSIS (electron-builder)

!macro customUnInstall
  ; przy aktualizacji (stary deinstalator uruchamiany w tle) dane użytkownika zostają nietknięte
  ${ifNot} ${isUpdated}
    IfSilent gran_skip_data
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Usunąć także pobrane dane map (kafle terenu, zapisane obszary) i ustawienia programu?$\r$\n$\r$\nWybierz „Nie”, aby zachować je na wypadek ponownej instalacji." /SD IDNO IDNO gran_skip_data
      SetShellVarContext current
      RMDir /r "$LOCALAPPDATA\GRAN"
      RMDir /r "$APPDATA\GRAŃ"
      SetShellVarContext all
    gran_skip_data:
  ${endIf}
!macroend
