{
  lib,
  stdenv,
  cmake,
  ninja,
  pkg-config,
  makeWrapper,
  go,
  alsa-lib,
  dbus,
  libei,
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
    libei
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
stdenv.mkDerivation {
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
        (type == "directory" && (name == "build" || name == "node_modules"))
        || lib.hasSuffix ".AppImage" name
      );
  };

  nativeBuildInputs = [
    cmake
    ninja
    pkg-config
    makeWrapper
    go
  ];

  buildInputs = runtimeLibraries ++ [
    libxcb
    wayland-protocols
  ];

  cmakeFlags = [
    "-DCMAKE_BUILD_TYPE=Release"
    "-DSMU_BUNDLE_SDL3=OFF"
    "-DSMU_ENABLE_SOURCE_TREE_FALLBACK=OFF"
    "-DSMU_LINK_SDL3_STATIC=ON"
  ];

  preBuild = ''
    export GOCACHE="$TMPDIR/go-cache"
    export GOTMPDIR="$TMPDIR/go-tmp"
    mkdir -p "$GOCACHE" "$GOTMPDIR"
  '';

  doCheck = true;
  checkPhase = ''
    runHook preCheck
    ctest --output-on-failure -R '^linux-updater$'
    runHook postCheck
  '';

  postInstall = ''
    appDir="$out/libexec/spencer-macro-utilities"
    mkdir -p "$appDir" "$out/bin" "$out/share/applications" \
      "$out/share/icons/hicolor/256x256/apps" "$out/share/doc/spencer-macro-utilities"

    mv "$out/suspend" "$appDir/suspend"
    mv "$out/assets" "$appDir/assets"
    mv "$out/nethelper" "$appDir/nethelper-unwrapped"

    if [ -d "$out/scripts" ]; then
      mv "$out/scripts" "$appDir/scripts"
    fi
    if [ -f "$out/LINUX_SETUP.md" ]; then
      cp "$out/LINUX_SETUP.md" "$appDir/LINUX_SETUP.md"
      mv "$out/LINUX_SETUP.md" "$out/share/doc/spencer-macro-utilities/LINUX_SETUP.md"
    fi
    if [ -f "$out/LICENSE" ]; then
      mv "$out/LICENSE" "$out/share/doc/spencer-macro-utilities/LICENSE"
    fi
    if [ -f "$out/PRIVACY.md" ]; then
      mv "$out/PRIVACY.md" "$out/share/doc/spencer-macro-utilities/PRIVACY.md"
    fi
    if [ -f "$out/THIRD_PARTY_NOTICES.md" ]; then
      mv "$out/THIRD_PARTY_NOTICES.md" \
        "$out/share/doc/spencer-macro-utilities/THIRD_PARTY_NOTICES.md"
    fi
    if [ -d "$out/licenses" ]; then
      mv "$out/licenses" "$out/share/doc/spencer-macro-utilities/licenses"
    fi
    rm -f "$out/run.sh"

    makeWrapper "$appDir/nethelper-unwrapped" "$appDir/nethelper" \
      --prefix PATH : "${runtimePath}"
    makeWrapper "$appDir/suspend" "$out/bin/spencer-macro-utilities" \
      --set SMU_APPDIR "$appDir" \
      --prefix PATH : "${runtimePath}" \
      --prefix LD_LIBRARY_PATH : "${runtimeLibraryPath}"
    ln -s spencer-macro-utilities "$out/bin/suspend"

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
