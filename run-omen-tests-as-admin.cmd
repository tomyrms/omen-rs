@echo off
rem Self-elevating launcher for the omen.exe live test script.
powershell -NoProfile -Command "Start-Process pwsh -Verb RunAs -Wait -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','%~dp0run-omen-tests.ps1'"
echo.
echo Results written to %~dp0live-test-result.txt
pause
