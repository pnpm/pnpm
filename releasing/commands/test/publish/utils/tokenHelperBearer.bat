@echo off
setlocal enabledelayedexpansion

set "PASSWORD=password"

for /f "delims=" %%i in ('powershell -Command "[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('!PASSWORD!'))"') do set ENCODED=%%i

echo Bearer %ENCODED%
