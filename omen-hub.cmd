@echo off
rem Self-elevating launcher for the OMEN Hub RS GUI.
powershell -NoProfile -Command "Start-Process '%~dp0target\debug\omen-hub.exe' -Verb RunAs"
