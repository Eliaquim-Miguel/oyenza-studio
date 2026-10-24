; Poni installer (NSIS 3). Per-user install, no admin rights needed.
;
; Build (Linux/macOS/Windows with makensis):
;   makensis -DEXE=/abs/path/poni.exe -DOUTDIR=/abs/path/dist installer/poni.nsi
; tools/package.sh does this for you.

; Native 64-bit installer (Poni itself is x64-only).
Target amd64-unicode
SetCompressor /SOLID lzma

!define APPNAME "Poni"
!define VERSION "0.1.1"
!define PUBLISHER "Jatevo Labs"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Poni"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

!ifndef EXE
  !error "Pass -DEXE=<path to poni.exe>"
!endif
!ifndef OUTDIR
  !define OUTDIR "."
!endif

Name "${APPNAME}"
OutFile "${OUTDIR}/Poni-Setup-${VERSION}.exe"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\Poni"
InstallDirRegKey HKCU "Software\Poni" "InstallDir"
BrandingText "${APPNAME} ${VERSION}"

VIProductVersion "0.1.1.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "Poni installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright (C) 2026 ${PUBLISHER}"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"

; Ask a running Poni to quit (it saves its data on WM_CLOSE), wait up to 3 s,
; then force-close with the fully qualified system taskkill.
!macro StopPoni
  FindWindow $0 "Poni.Island"
  ${If} $0 <> 0
    SendMessage $0 ${WM_CLOSE} 0 0 /TIMEOUT=2000
    StrCpy $1 0
    ${DoWhile} $0 <> 0
      ${If} $1 >= 30
        ${Break}
      ${EndIf}
      Sleep 100
      FindWindow $0 "Poni.Island"
      IntOp $1 $1 + 1
    ${Loop}
  ${EndIf}
  nsExec::Exec '"$SYSDIR\taskkill.exe" /IM poni.exe /F'
  Sleep 200
!macroend
!define MUI_ICON "../crates/poni/res/poni.ico"
!define MUI_UNICON "../crates/poni/res/poni.ico"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_FINISHPAGE_RUN "$INSTDIR\poni.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Poni now"
!define MUI_WELCOMEPAGE_TITLE "Welcome to Poni"
!define MUI_WELCOMEPAGE_TEXT "Poni puts a living notch at the top of your screen: music with synced lyrics, a focus timer, water reminders, a file shelf, tasks, clipboard history and more.$\r$\n$\r$\nIt installs just for you and doesn't need administrator rights."

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Poni" SecMain
  SectionIn RO
  ; Close a running copy so the exe can be replaced.
  !insertmacro StopPoni
  SetOutPath "$INSTDIR"
  File "/oname=poni.exe" "${EXE}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Poni.lnk" "$INSTDIR\poni.exe" "" "$INSTDIR\poni.exe" 0
  WriteRegStr HKCU "Software\Poni" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\poni.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" 2048
SectionEnd

Section "Start Poni with Windows" SecAutostart
  ; Same value the app's own "Start with Windows" toggle writes.
  WriteRegStr HKCU "${RUN_KEY}" "Poni" '"$INSTDIR\poni.exe"'
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "The Poni app and a Start Menu shortcut."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAutostart} "Open Poni automatically when you sign in. You can change this later from Poni's menu."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  !insertmacro StopPoni
  Delete "$INSTDIR\poni.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Poni.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "Poni"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\Poni"
  ; Cache and log (lyrics lookups name the songs you played) go too.
  RMDir /r "$LOCALAPPDATA\Poni"
  ; Your settings and data (tasks, water log, shelf) in %APPDATA%\Poni are kept.
SectionEnd
