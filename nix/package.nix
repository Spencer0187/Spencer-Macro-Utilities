{
  lib,
  rustPlatform,
  cmake,
  ninja,
  pkg-config,
  makeWrapper,
  go,
  alsa-lib,
  dbus,
  libdecor,
  libGL,
  libpulseaudio,
  libxcb,
  libxkbcommon,
  udev,
  wayland,
  wayland-protocols,
  xorg,
  curl,
  iproute2,
  iptables,
  kmod,
  polkit,
  pipewire,
  xdg-desktop-portal,
  zenity,
}:

let
  rawVersion = builtins.readFile ../version;
  version = lib.removeSuffix "\r" (lib.removeSuffix "\n" rawVersion);
  runtimeTools = [
    curl
    iproute2
    iptables
    kmod
    polkit
    xdg-desktop-portal
    zenity
  ];
  runtimeLibraries = [
    alsa-lib
    dbus
    libdecor
    libGL
    libpulseaudio
    pipewire
    libxkbcommon
    udev
    wayland
    xorg.libX11
    xorg.libXcursor
    xorg.libXext
    xorg.libXfixes
    xorg.libXi
    xorg.libXinerama
    xorg.libXrandr
    xorg.libXrender
    xorg.libXScrnSaver
    xorg.libXtst
  ];
  runtimePath = lib.makeBinPath runtimeTools;
  runtimeLibraryPath = lib.makeLibraryPath runtimeLibraries;
in
rustPlatform.buildRustPackage {
  pname = "spencer-macro-utilities";
  inherit version;

  src = lib.cleanSourceWith {
    src = lib.cleanSource ../.;
    filter =
      path: type:
      let
        name = baseNameOf path;
      in
      !(
        (type == "directory" && (name == "build" || name == "node_modules" || name == "target"))
        || lib.hasSuffix ".AppImage" name
      );
  };

  cargoLock.lockFile = ../rust/Cargo.lock;
  cargoRoot = "rust";
  buildAndTestSubdir = "rust";
  cargoBuildFlags = [ "--package=smu-app" ];
  cargoTestFlags = [ "--package=smu-app" ];

  nativeBuildInputs = [
    cmake
    ninja
    pkg-config
    makeWrapper
    go
    rustPlatform.bindgenHook
  ];

  buildInputs = runtimeLibraries ++ [
    libxcb
    wayland-protocols
  ];

  postBuild = ''
    root="$PWD"
    if [ ! -d "$root/platform/linux/nethelper" ]; then
      root="$(dirname "$root")"
    fi
    if [ ! -d "$root/platform/linux/nethelper" ]; then
      echo "Could not locate the repository root from $PWD" >&2
      exit 1
    fi

    helperOut="$TMPDIR/smu-helpers"
    export GOCACHE="$TMPDIR/go-cache"
    export GOTMPDIR="$TMPDIR/go-tmp"
    export GOPROXY=off
    export GOSUMDB=off
    mkdir -p "$helperOut" "$GOCACHE" "$GOTMPDIR"

    (
      cd "$root/platform/linux/nethelper"
      go test -mod=vendor ./...
      CGO_ENABLED=0 go build \
        -mod=vendor \
        -trimpath \
        -buildvcs=false \
        -ldflags='-s -w' \
        -o "$helperOut/nethelper" \
        .
    )
    (
      cd "$root/platform/linux/processhelper"
      go test ./...
      CGO_ENABLED=0 go build \
        -trimpath \
        -buildvcs=false \
        -ldflags='-s -w' \
        -o "$helperOut/processhelper" \
        .
    )

    mapfile -t sdlLinks < <(
      find "$root" -path '*/release/build/sdl3-sys-*/out/lib64/libSDL3.so.0' -type l -print
    )
    if [ "''${#sdlLinks[@]}" -ne 1 ]; then
      echo "Expected exactly one Cargo-built SDL3 soname link, found ''${#sdlLinks[@]}" >&2
      printf '  %s\n' "''${sdlLinks[@]}" >&2
      exit 1
    fi
    sdlReal="$(readlink -f "''${sdlLinks[0]}")"
    sdlName="$(basename "$sdlReal")"
    cp "$sdlReal" "$helperOut/$sdlName"
    printf '%s\n' "$sdlName" > "$helperOut/sdl-name"
  '';

  doCheck = true;

  postInstall = ''
    appDir="$out/libexec/spencer-macro-utilities"
    helperOut="$TMPDIR/smu-helpers"
    mkdir -p \
      "$appDir/assets" \
      "$appDir/scripts" \
      "$out/bin" \
      "$out/lib" \
      "$out/share/applications" \
      "$out/share/icons/hicolor/256x256/apps" \
      "$out/share/doc/spencer-macro-utilities/licenses"

    install -Dm755 "$out/bin/smu-app" "$appDir/Spencer-Macro-Utilities"
    rm -f "$out/bin/smu-app"

    cp -R "$src/assets/." "$appDir/assets/"
    install -Dm755 "$src/scripts/install_linux_permissions.sh" \
      "$appDir/scripts/install_linux_permissions.sh"
    install -Dm755 "$helperOut/nethelper" "$appDir/nethelper-unwrapped"
    install -Dm755 "$helperOut/processhelper" "$appDir/processhelper"

    install -Dm644 "$src/LINUX_SETUP.md" "$appDir/LINUX_SETUP.md"
    install -Dm644 "$src/LINUX_SETUP.md" \
      "$out/share/doc/spencer-macro-utilities/LINUX_SETUP.md"
    install -Dm644 "$src/LICENSE" "$out/share/doc/spencer-macro-utilities/LICENSE"
    install -Dm644 "$src/PRIVACY.md" "$out/share/doc/spencer-macro-utilities/PRIVACY.md"
    install -Dm644 "$src/THIRD_PARTY_NOTICES.md" \
      "$out/share/doc/spencer-macro-utilities/THIRD_PARTY_NOTICES.md"
    install -Dm644 "$src/third_party/SDL/LICENSE.txt" \
      "$out/share/doc/spencer-macro-utilities/licenses/SDL.txt"
    install -Dm644 "$src/third_party/appimage-runtime/LICENSE" \
      "$out/share/doc/spencer-macro-utilities/licenses/AppImage-type2-runtime.txt"
    install -Dm644 "$src/platform/linux/nethelper/vendor/github.com/coreos/go-iptables/LICENSE" \
      "$out/share/doc/spencer-macro-utilities/licenses/go-iptables-LICENSE.txt"
    install -Dm644 "$src/platform/linux/nethelper/vendor/github.com/coreos/go-iptables/NOTICE" \
      "$out/share/doc/spencer-macro-utilities/licenses/go-iptables-NOTICE.txt"
    install -Dm644 "$src/third_party/windivert/LICENSE-LGPL-3.0.txt" \
      "$out/share/doc/spencer-macro-utilities/licenses/WinDivert-LGPL-3.0.txt"

    sdlName="$(cat "$helperOut/sdl-name")"
    install -Dm755 "$helperOut/$sdlName" "$out/lib/$sdlName"
    ln -s "$sdlName" "$out/lib/libSDL3.so.0"
    ln -s "$sdlName" "$out/lib/libSDL3.so"

    makeWrapper "$appDir/nethelper-unwrapped" "$appDir/nethelper" \
      --prefix PATH : "${runtimePath}"
    makeWrapper "$appDir/Spencer-Macro-Utilities" "$out/bin/spencer-macro-utilities" \
      --set SMU_APPDIR "$appDir" \
      --prefix PATH : "${runtimePath}" \
      --prefix LD_LIBRARY_PATH : "$out/lib:${runtimeLibraryPath}"

    install -Dm644 "$src/AppImage/SMU.png" \
      "$out/share/icons/hicolor/256x256/apps/spencer-macro-utilities.png"
    install -Dm644 "$src/AppImage/SMU.desktop" \
      "$out/share/applications/spencer-macro-utilities.desktop"
  '';

  passthru.runtimeDependencies = runtimeTools ++ runtimeLibraries;

  meta = {
    description = "Cross-platform macro utility with a custom scripting API";
    homepage = "https://github.com/Spencer0187/Spencer-Macro-Utilities";
    license = lib.licenses.gpl3Only;
    mainProgram = "spencer-macro-utilities";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
}
