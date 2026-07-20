@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if errorlevel 1 exit /b 1
set "LIBCLANG_PATH=C:\Program Files\LLVM\bin"
set "PATH=C:\Program Files\CMake\bin;C:\Program Files\LLVM\bin;%PATH%"
set "CARGO_INCREMENTAL=0"
cd /d "%~dp0"
cd /d "%~dp0..\..\.."
echo Cleaning workspace target...
cargo clean
cd /d "%~dp0"
pnpm tauri dev
