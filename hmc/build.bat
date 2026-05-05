@echo off

set CommonCompilerFlags=-MTd -nologo -Gm- -GR- -EHa- -Od -Oi -WX -W4 -wd4201 -wd4100 -wd4189 -wd4505 -DDEBUG_PROFILE=1 -DDEBUG_ASSERTIONS=1 -DHANDMADE_WIN32=0 -FC -Z7
set CommonLinkerFlags= -incremental:no -opt:ref user32.lib gdi32.lib winmm.lib
set ROOT_DIR=%CD:\=/%

REM TODO - can we just build both with one exe?

IF NOT EXIST .\build mkdir .\build
(
echo [
echo   {
echo     "directory": "%ROOT_DIR%",
echo     "command": "cl.exe %CommonCompilerFlags% -I./src /c src/handmade.c /Fobuild/handmade.obj",
echo     "file": "src/handmade.c"
echo   },
echo   {
echo     "directory": "%ROOT_DIR%",
echo     "command": "cl.exe %CommonCompilerFlags% -I./src /c src/win32_handmade.c /Fobuild/win32_handmade.obj",
echo     "file": "src/win32_handmade.c"
echo   }
echo ]
) > compile_commands.json
pushd .\build

REM REM 32-bit build
REM REM cl %CommonCompilerFlags% ..\src\win32_handmade.c /link -subsystem:windows,5.1 %CommonLinkerFlags%

REM REM 64-bit build
del *.pdb > NUL 2> NUL
cl %CommonCompilerFlags% ..\src\handmade.c -Fmhandmade.map -LD /link -incremental:no -opt:ref -PDB:handmade_%random%.pdb -EXPORT:game_get_sound_samples -EXPORT:game_update_and_render
cl %CommonCompilerFlags% ..\src\win32_handmade.c -Fmwin32_handmade.map /link %CommonLinkerFlags%
popd
