Mesa's software OpenGL for McGyver

McGyver draws its wheel with OpenGL 2.0 or later. Where Windows has only its own OpenGL 1.1
(a virtual machine without 3D acceleration, Remote Desktop, a PC without its graphics driver),
McGyver starts again from this folder and draws with these files instead: Mesa's llvmpipe,
which runs on the processor. With a graphics driver they are never used. Deleting this folder
only takes that fallback away.

  opengl32.dll, libgallium_wgl.dll
  Mesa 26.2.4, as built by mesa-dist-win: https://github.com/pal1000/mesa-dist-win
  Source: https://gitlab.freedesktop.org/mesa/mesa

Mesa is under the MIT licence (https://docs.mesa3d.org/license.html); mesa-dist-win too. The
build includes LLVM, under the Apache License 2.0 with LLVM Exceptions
(https://llvm.org/LICENSE.txt).

McGyver may also put a link to McGyver.exe in this folder, the first time it needs the fallback:
it is the same file, not a copy.
