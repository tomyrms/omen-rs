@echo off
rem Optional launcher for the OMEN Hub RS GUI.
rem omen-hub.exe embeds a requireAdministrator manifest, so even a plain
rem double-click triggers the UAC prompt by itself - this file exists only
rem as a convenience alias.
start "" "%~dp0target\debug\omen-hub.exe"
