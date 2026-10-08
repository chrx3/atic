; Instalador de Atic para Windows, sin Tauri: la app es la pill GPUI
; (atic-pill.exe) y sus sidecars (atic-mcp, atic-unix).
;
; Compatible con las instalaciones que hizo el instalador de Tauri:
; - misma carpeta (%LOCALAPPDATA%\Atic) y mismas claves de registro
;   (Uninstall\Atic y Software\chrx3\Atic), así se instala encima;
; - acepta los argumentos con que el actualizador de Tauri de las versiones
;   viejas lo lanza: /P (pasivo), /R (reabrir al terminar), /UPDATE y /ARGS
;   (se ignoran);
; - borra atic-desktop.exe (la app de Tauri) al actualizar.
;
; Lo compila scripts/build-installer.ps1 con:
;   makensis /DVERSION=x.y.z /DSRC=<carpeta con los exe> /DOUT=<salida>

Unicode true
SetCompressor /SOLID lzma

!ifndef VERSION
  !error "Falta /DVERSION"
!endif
!ifndef SRC
  !error "Falta /DSRC"
!endif
!ifndef OUT
  !define OUT "Atic_${VERSION}_x64-setup.exe"
!endif

!define PRODUCT "Atic"
!define PUBLISHER "chrx3"
!define MAIN "atic-pill.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"
!define DIR_KEY "Software\${PUBLISHER}\${PRODUCT}"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

Name "${PRODUCT}"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\${PRODUCT}"
InstallDirRegKey HKCU "${DIR_KEY}" ""
RequestExecutionLevel user
ShowInstDetails nevershow
ShowUninstDetails nevershow
BrandingText "${PRODUCT} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "${PRODUCT}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define MUI_ICON "${SRC}\icon.ico"
!define MUI_UNICON "${SRC}\icon.ico"
!define MUI_ABORTWARNING

Var Passive

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!define MUI_FINISHPAGE_RUN "$INSTDIR\${MAIN}"
!define MUI_FINISHPAGE_RUN_PARAMETERS "--native"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "Spanish"
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  StrCpy $Passive 0
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/P" $R1
  ${IfNot} ${Errors}
    StrCpy $Passive 1
    SetAutoClose true
  ${EndIf}
FunctionEnd

Function SkipIfPassive
  ${If} $Passive == 1
    Abort
  ${EndIf}
FunctionEnd

; Un exe en uso no se puede sobrescribir, pero sí renombrar: el sidecar MCP
; suele estar abierto por los agentes del usuario. La copia vieja se borra en
; la próxima instalación, cuando ya nadie la use.
!macro ReplaceInUse NAME
  Delete "$INSTDIR\${NAME}.old-*.exe"
  ${If} ${FileExists} "$INSTDIR\${NAME}.exe"
    Delete "$INSTDIR\${NAME}.exe"
    ${If} ${FileExists} "$INSTDIR\${NAME}.exe"
      System::Call "kernel32::GetTickCount() i .r9"
      Rename "$INSTDIR\${NAME}.exe" "$INSTDIR\${NAME}.old-$9.exe"
    ${EndIf}
  ${EndIf}
!macroend

Section "Atic"
  SetOutPath "$INSTDIR"

  ; La pill (y la app de Tauri de versiones viejas) tienen sus exe abiertos.
  nsExec::Exec 'taskkill /F /IM atic-pill.exe'
  nsExec::Exec 'taskkill /F /IM atic-desktop.exe'
  Sleep 300

  !insertmacro ReplaceInUse "atic-pill"
  !insertmacro ReplaceInUse "atic-mcp"
  !insertmacro ReplaceInUse "atic-unix"
  File "${SRC}\atic-pill.exe"
  File "${SRC}\atic-mcp.exe"
  File "${SRC}\atic-unix.exe"

  ; Lo que dejaba la versión con Tauri.
  Delete "$INSTDIR\atic-desktop.exe"
  Delete "$INSTDIR\atic-desktop.exe.bak"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "${DIR_KEY}" "" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\${MAIN}"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" '"$INSTDIR"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "MainBinaryName" "${MAIN}"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0

  ; `--native` enciende native_pill en config.json si estaba apagado. El inicio
  ; con Windows lo escribe la pill al arrancar.
  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${MAIN}" "--native" "$INSTDIR\${MAIN}" 0

  ; Pasivo o silencioso (actualización): no hay página final, así que se
  ; reabre aquí.
  ${If} $Passive == 1
  ${OrIf} ${Silent}
    Exec '"$INSTDIR\${MAIN}" --native'
  ${EndIf}
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /F /IM atic-pill.exe'
  Sleep 300
  Delete "$INSTDIR\atic-pill.exe"
  Delete "$INSTDIR\atic-mcp.exe"
  Delete "$INSTDIR\atic-unix.exe"
  Delete "$INSTDIR\atic-*.old-*.exe"
  Delete "$INSTDIR\atic-desktop.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$DESKTOP\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "${DIR_KEY}"
  DeleteRegValue HKCU "${RUN_KEY}" "${PRODUCT}"
  ; Los datos (grabaciones, config) quedan en %APPDATA%\ciat\atic.
SectionEnd
