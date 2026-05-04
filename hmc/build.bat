@echo off

set CommonCompilerFlags=-MTd -nologo -Gm- -GR- -EHa- -Od -Oi -WX -W4 -wd4201 -wd4100 -wd4189 -wd4505 -DDEBUG_PROFILE=1 -DDEBUG_ASSERTIONS=1 -DHANDMADE_WIN32=0 -FC -Z7
set CommonLinkerFlags= -incremental:no -opt:ref user32.lib gdi32.lib winmm.lib

REM TODO - can we just build both with one exe?

IF NOT EXIST .\build mkdir .\build
pushd .\build

REM REM 32-bit build
REM REM cl %CommonCompilerFlags% ..\src\win32_handmade.c /link -subsystem:windows,5.1 %CommonLinkerFlags%

REM REM 64-bit build
del *.pdb > NUL 2> NUL
cl %CommonCompilerFlags% ..\src\handmade.c -Fmhandmade.map -LD /link -incremental:no -opt:ref -PDB:handmade_%random%.pdb -EXPORT:game_get_sound_samples -EXPORT:game_update_and_render
cl %CommonCompilerFlags% ..\src\win32_handmade.c -Fmwin32_handmade.map /link %CommonLinkerFlags%
popd
