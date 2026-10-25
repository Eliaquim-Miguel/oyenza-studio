@echo off
rem Double-click: measures whether Oyen Poni's readout lets the game send its
rem frames straight to the screen (see overlay-check.ps1). It asks Windows for
rem admin once, because PresentMon needs it.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0overlay-check.ps1"
