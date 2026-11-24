@echo off
rem Builds SaxonDotnetHost.exe into bin\ with the .NET Framework C# compiler
rem that ships with Windows, against your copy of the Saxon-HE .NET assemblies.
rem
rem   build.cmd "C:\path\to\Saxon-HE.9.7.0.21\lib\net40"
rem
rem The Saxon assemblies are copied next to the program. They are not part of
rem this repository.

setlocal
set SAXON=%~1
if "%SAXON%"=="" (
  echo Usage: build.cmd ^<folder holding saxon9he-api.dll^>
  exit /b 2
)
if not exist "%SAXON%\saxon9he-api.dll" (
  echo saxon9he-api.dll was not found in %SAXON%
  exit /b 1
)
set CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe
if not exist "%CSC%" set CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe
set HERE=%~dp0
if not exist "%HERE%bin" mkdir "%HERE%bin"
copy /y "%SAXON%\*.dll" "%HERE%bin\" > nul
"%CSC%" /nologo /target:exe /optimize+ /out:"%HERE%bin\SaxonDotnetHost.exe" ^
  /reference:"%HERE%bin\saxon9he-api.dll" /reference:"%HERE%bin\saxon9he.dll" ^
  /reference:"%HERE%bin\IKVM.Runtime.dll" /reference:"%HERE%bin\IKVM.OpenJDK.Core.dll" ^
  /reference:System.Xml.dll /reference:System.Xml.Linq.dll /reference:System.Core.dll ^
  "%HERE%SaxonDotnetHost.cs"
exit /b %ERRORLEVEL%
