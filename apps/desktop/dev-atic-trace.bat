@echo off
REM Igual que dev-atic.bat pero con las trazas de diagnostico encendidas.
REM Ver README, seccion "Depurar la pill y el pegado".
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if errorlevel 1 exit /b 1
set "LIBCLANG_PATH=C:\Program Files\LLVM\bin"
set "PATH=C:\Program Files\CMake\bin;C:\Program Files\LLVM\bin;%PATH%"
set "RUST_LOG=info,pill_geo=debug,paste_geo=debug"
cd /d "%~dp0"
pnpm tauri dev
