@echo off
rem Oratio dev launcher for Windows ARM64.
rem whisper.cpp/ggml refuses MSVC on ARM64 — build it with clang-cl + Ninja.

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsarm64.bat"
if errorlevel 1 (
    echo Could not find vcvarsarm64.bat - check the VS Build Tools install path.
    exit /b 1
)

set "PATH=C:\Program Files\nodejs;C:\Program Files\LLVM\bin;C:\Program Files\CMake\bin;%PATH%"
set CMAKE_GENERATOR=Ninja
set CC=clang-cl
set CXX=clang-cl
rem ggml (whisper.cpp <=1.7.x) casts u16* to __fp16* in its NEON f16 loads on
rem windows-arm64. The cast is bit-correct, but clang 22 treats the pointer
rem mismatch as an ERROR by default — demote it back to a warning.
set CFLAGS=-Wno-incompatible-pointer-types
rem /EHsc: clang-cl disables C++ exceptions by default, whisper.cpp needs them.
set CXXFLAGS=-Wno-incompatible-pointer-types /EHsc

cd /d "%~dp0..\apps\desktop"
npm run tauri dev
