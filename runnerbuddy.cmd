@echo off
setlocal

cd /d "%~dp0"

set TAURI_BIN=node_modules\.bin\tauri.cmd

if "%~1"=="--check" (
  if not exist "%TAURI_BIN%" (
    if exist package.json (
      npm install
    )
  )
  echo ok
  exit /b 0
)

if not exist "%TAURI_BIN%" (
  if exist package.json (
    npm install
  )
)

call "%TAURI_BIN%" dev
