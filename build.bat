@echo off
echo === RTK Windows Build (MSVC) ===

:: Setup MSVC environment
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat" x64
if errorlevel 1 (
    echo ERROR: Failed to setup MSVC environment
    echo Install "Desktop development with C++" in Visual Studio Installer
    exit /b 1
)

cd /d D:\rtk

:: Build release
echo Building rtk in release mode...
cargo build --release
if errorlevel 1 (
    echo ERROR: Build failed
    exit /b 1
)

:: Show result
echo.
echo === Build successful ===
target\release\rtk.exe --version
for %%I in (target\release\rtk.exe) do echo Binary: %%~fI (%%~zI bytes)
echo.

:: Optional: install to cargo bin
set /p INSTALL="Install to cargo bin? (y/N): "
if /i "%INSTALL%"=="y" (
    cargo install --path .
    echo Installed!
)
