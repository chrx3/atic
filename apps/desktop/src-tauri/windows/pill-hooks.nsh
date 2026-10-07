; Hooks del instalador con la pill nativa (tauri.pill.conf.json).
;
; La pill GPUI (atic-pill.exe) es el proceso principal en Windows: el acceso
; del menú Inicio la abre a ella, y ella abre la ventana de Atic cuando hace
; falta. Su inicio con Windows lo escribe la propia pill al arrancar (valor
; "Atic" de Run, el mismo que usaba Tauri).

!macro NSIS_HOOK_PREINSTALL
  ; Una pill corriendo bloquea sobrescribir su exe al actualizar.
  nsExec::Exec 'taskkill /F /IM atic-pill.exe'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; `--native` enciende native_pill en config.json si estaba apagado.
  CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\atic-pill.exe" "--native" "$INSTDIR\${MAINBINARYNAME}.exe" 0
  ; Arranca la pill (y no la ventana de Tauri) al terminar de instalar.
  Exec '"$INSTDIR\atic-pill.exe" --native'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'taskkill /F /IM atic-pill.exe'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Atic"
!macroend
