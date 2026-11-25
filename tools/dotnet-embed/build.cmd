@echo off
rem Builds XylemDotnetHost.exe into bin\ with the .NET Framework C# compiler
rem that ships with Windows, and copies Xylem's XSLT library next to it.
rem
rem   cargo build --release -p xylem-xslt-ffi
rem   build.cmd [folder holding xylem_xslt.dll]
rem
rem The folder defaults to the repository's target\release.
rem
rem The program is built AnyCPU without a 32-bit preference, as the .NET
rem publishing service it stands in for is, so on 64-bit Windows it runs as a
rem 64-bit process and needs the 64-bit (x86_64-pc-windows-msvc) library.

setlocal
set HERE=%~dp0
set LIBDIR=%~1
if "%LIBDIR%"=="" set LIBDIR=%HERE%..\..\target\release
if not exist "%LIBDIR%\xylem_xslt.dll" (
  echo xylem_xslt.dll was not found in %LIBDIR%
  echo Build it first: cargo build --release -p xylem-xslt-ffi
  exit /b 1
)
set CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe
if not exist "%CSC%" set CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe
if not exist "%HERE%bin" mkdir "%HERE%bin"
copy /y "%LIBDIR%\xylem_xslt.dll" "%HERE%bin\" > nul
"%CSC%" /nologo /target:exe /platform:anycpu /optimize+ /out:"%HERE%bin\XylemDotnetHost.exe" ^
  /reference:System.Xml.dll /reference:System.Xml.Linq.dll /reference:System.Core.dll ^
  "%HERE%XylemDotnetHost.cs"
exit /b %ERRORLEVEL%
