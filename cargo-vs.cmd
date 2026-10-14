@echo off
rem Wrapper: load the Visual Studio x64 build environment, then run cargo.
rem Needed because MSYS2's GNU link.exe shadows MSVC's link.exe on PATH.
call "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
"%USERPROFILE%\.cargo\bin\cargo.exe" %*
