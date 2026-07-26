# Third-party notices

Spencer Macro Utilities includes the following open-source components. The
project's own license is in `LICENSE`.

## SDL 3.4.4

Copyright (C) 1997-2026 Sam Lantinga <slouken@libsdl.org>

SDL is distributed under the zlib license:

> This software is provided "as-is", without any express or implied warranty.
> In no event will the authors be held liable for any damages arising from the
> use of this software.
>
> Permission is granted to anyone to use this software for any purpose,
> including commercial applications, and to alter it and redistribute it
> freely, subject to the following restrictions:
>
> 1. The origin of this software must not be misrepresented; you must not claim
>    that you wrote the original software. If you use this software in a
>    product, an acknowledgment in the product documentation would be
>    appreciated but is not required.
> 2. Altered source versions must be plainly marked as such, and must not be
>    misrepresented as being the original software.
> 3. This notice may not be removed or altered from any source distribution.

The exact vendored notice is also available at `third_party/SDL/LICENSE.txt`
in the source repository.

## AppImage type-2 runtime

The Linux AppImage embeds the official x86_64 AppImage type-2 runtime from
commit `75849dce7cc37e4319b633df1f116ca895c71a12`. The runtime is distributed
under the MIT license and statically incorporates the additional components
listed in its upstream license notice.

Release packages include that notice as
`licenses/AppImage-type2-runtime.txt`. The pinned upstream runtime asset has
SHA-256
`1cc49bcf1e2ccd593c379adb17c9f85a36d619088296504de95b1d06215aebbf`.
Its corresponding source and build history are available from:

- https://github.com/AppImage/type2-runtime/tree/75849dce7cc37e4319b633df1f116ca895c71a12
- https://github.com/AppImage/type2-runtime/releases/tag/continuous

## MIT-licensed components

The following components are distributed under the MIT license:

- Dear ImGui 1.92.5: Copyright (c) 2014-2025 Omar Cornut and contributors.
- Dear ImGui's stb-derived headers: Copyright (c) 2017 Sean Barrett.
- ImGuiFileDialog 0.6.9: Copyright (c) 2018-2025 Stephane Cuillerdier
  (aiekick).
- Lua 5.4.8: Copyright (C) 1994-2025 Lua.org, PUC-Rio.
- JSON for Modern C++ 3.11.3: Copyright (c) 2013-2023 Niels Lohmann.
- JSON for Modern C++'s UTF-8 decoder: Copyright (c) 2008-2009
  Bjoern Hoehrmann.
- JSON for Modern C++'s Grisu2 implementation: Copyright (c) 2009
  Florian Loitsch.
- miniz 3.0.0 portions: Copyright 2013-2014 RAD Game Tools and Valve
  Software; Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC;
  Copyright 2016 Martin Raiber.

MIT license:

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to
> deal in the Software without restriction, including without limitation the
> rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
> sell copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.

JSON for Modern C++ also includes Hedley under CC0-1.0 and small Apache-2.0
portions from Abseil. The exact notices remain in the vendored amalgamated
header at `third_party/json/json.hpp`.

Some miniz portions are additionally dedicated to the public domain under the
Unlicense. Their exact notices remain in `third_party/miniz/miniz.c`.

## go-iptables 0.8.0

The Linux network helper vendors `github.com/coreos/go-iptables` under the
Apache License 2.0.

CoreOS Project
Copyright 2018 CoreOS, Inc.

This product includes software developed at CoreOS, Inc.
(http://www.coreos.com/).

Release packages include the complete Apache 2.0 license and CoreOS NOTICE
files. They are also available in the source tree at
`platform/linux/nethelper/vendor/github.com/coreos/go-iptables/LICENSE` and
`platform/linux/nethelper/vendor/github.com/coreos/go-iptables/NOTICE`.

## WinDivert 2.2.2

Windows packages embed the unmodified official x64 files from the
WinDivert 2.2.2-A distribution. The vendored DLL, driver, import library, and
header have been byte-compared with that official archive.

WinDivert is dual-licensed under the user's choice of the GNU Lesser General
Public License version 3 (or later) or the GNU General Public License version 2
(or later). SMU uses it under the LGPL version 3-or-later option.
Release packages include the LGPL additional terms as
`licenses/WinDivert-LGPL-3.0.txt`; the incorporated GPLv3 terms are in
`LICENSE`.

The official binary distribution, corresponding source archive, complete
license, and build instructions are available from:

- https://reqrypt.org/windivert.html
- https://reqrypt.org/download/WinDivert-2.2.2-A.zip
- https://reqrypt.org/download/WinDivert-2.2.2-Source.zip
- https://github.com/basil00/WinDivert/tree/v2.2.2

The official 2.2.2-A files embedded by SMU have these SHA-256 digests:

```text
c1e060ee19444a259b2162f8af0f3fe8c4428a1c6f694dce20de194ac8d7d9a2  WinDivert.dll
8da085332782708d8767bcace5327a6ec7283c17cfb85e40b03cd2323a90ddc2  WinDivert64.sys
c5678d544eb0121a189d1139f54e0c67854dc64d1c897111a27ef2e52cb38eb3  WinDivert.lib
5017a1768c1592fd664c0c2d3d2d30f81fad4ab98d322b1914c6f0a33fcacdf9  windivert.h
```

## Source

Corresponding SMU source, including vendored build inputs and the exact
dependency revisions used by each release, is available at:

https://github.com/Spencer0187/Spencer-Macro-Utilities
