@echo off
setlocal

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tinytop.ps1" %*
exit /b %ERRORLEVEL%
