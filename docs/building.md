# Building Spencer Macro Utilities

The cross-platform build entry point is:

```bash
python3 scripts/build.py
```

It creates an unsigned Windows executable, a portable Linux folder, or a universal macOS package depending on the host.

## Windows

Install Visual Studio 2022 with the **Desktop development with C++** workload, CMake, and Python 3. Then run:

```powershell
python scripts/build.py
```

Output:

```text
build/windows-local/Release/suspend.exe
```

Local Windows builds are intentionally unsigned. Official release builds are signed through SignPath in GitHub Actions.

## Linux

Go 1.18 or newer is required because Linux packages include the native
network lag-switch helper. CMake 3.23 or newer is required; Ubuntu 22.04's
default CMake is older, so install a current CMake first:

```bash
python3 -m pip install --user 'cmake>=3.23,<4'
export PATH="$HOME/.local/bin:$PATH"
```

### Ubuntu and Debian

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential curl golang-go pkg-config python3-pip unzip zip \
  libdecor-0-dev libwayland-dev wayland-protocols \
  libgl1-mesa-dev libx11-dev libxext-dev libxrandr-dev \
  libxcursor-dev libxfixes-dev libxi-dev libxss-dev \
  libxtst-dev libxinerama-dev libxkbcommon-dev
```

### Fedora

```bash
sudo dnf install -y \
  gcc gcc-c++ make cmake curl golang pkgconf-pkg-config unzip zip \
  libdecor-devel wayland-devel wayland-protocols-devel \
  mesa-libGL-devel libX11-devel libXext-devel libXrandr-devel \
  libXcursor-devel libXfixes-devel libXi-devel libXScrnSaver-devel \
  libXtst-devel libXinerama-devel libxkbcommon-devel
```

### Arch Linux and Manjaro

```bash
sudo pacman -Syu --needed \
  base-devel cmake curl go pkgconf unzip zip \
  libdecor wayland wayland-protocols \
  libglvnd mesa libx11 libxext libxrandr libxcursor \
  libxfixes libxi libxss libxtst libxinerama libxkbcommon
```

### openSUSE

```bash
sudo zypper install -y \
  -t pattern devel_basis
sudo zypper install -y \
  cmake curl go pkgconf unzip zip Mesa-libGL-devel \
  libdecor-devel wayland-devel wayland-protocols-devel \
  libX11-devel libXext-devel libXrandr-devel libXcursor-devel \
  libXfixes-devel libXi-devel libXScrnSaver-devel libXtst-devel \
  libXinerama-devel libxkbcommon-devel
```

### Package commands

Build the portable folder:

```bash
python3 scripts/build.py
```

Build an AppImage:

```bash
python3 scripts/build.py --appimage
```

If `appimagetool` is not installed, the AppImage command downloads the
official 1.9.1 x86_64 tool once into `build/tools`, verifies its pinned
SHA-256 digest, and verifies the cached copy again on every build. The exact
official type-2 runtime is pinned and verified separately, so appimagetool
does not silently fetch a newer mutable runtime during packaging.

Build the same all-in-one Linux ZIP used by releases:

```bash
go install github.com/goreleaser/nfpm/v2/cmd/nfpm@v2.47.0
export PATH="$(go env GOPATH)/bin:$PATH"
python3 scripts/build.py --linux-release
```

Output:

```text
dist/release/Spencer-Macro-Utilities-V<version>-Linux-x86_64.zip
```

You can pass extra CMake configure options with repeated `--cmake-arg` arguments.

## macOS

Install the Xcode Command Line Tools:

```bash
xcode-select --install
```

Build the universal Apple Silicon and Intel package:

```bash
python3 scripts/build.py
```

The local build uses ad-hoc signing unless `SMU_MACOS_SIGN_IDENTITY` names another available identity. Outputs are staged under:

```text
out/build/macos-universal-release/package-macos/
```

## Nix

With Nix flakes enabled:

```bash
nix build
nix run
nix flake check
```

The flake builds from the current checkout and uses the committed lock file.

For NixOS, add this repository as an input, include `smu.nixosModules.default`, and configure the desktop users who should receive input-device access:

```nix
programs.spencer-macro-utilities = {
  enable = true;
  inputUsers = [ "your-user-name" ];
};
```

See [nix-flake-review-v3.3.0.md](nix-flake-review-v3.3.0.md) for the external proposal review, the integrated design, and the remaining physical validation.

## Checks

Validate version metadata:

```bash
python3 scripts/version.py --check
```

Run repository tests:

```bash
python3 -m unittest discover -s tests
```

The optional precise-sleep benchmark sweeps every threshold in a range while
calling a Lua `sleepMicros()` loop, writes per-call timing/CPU samples to CSV,
and prints a recommended spin threshold. The recommendation is the lowest
measured-CPU threshold whose p99 absolute timing error is within the configured
`--p99-error-us` limit (2 us by default). It is disabled by default because a
full sweep is intentionally long and should be run on each target machine when
comparing platforms:

```bash
cmake -S . -B build/sleep-benchmark \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_TESTING=ON \
  -DSMU_BUILD_SLEEP_BENCHMARK=ON
cmake --build build/sleep-benchmark \
  --target smu_precise_sleep_benchmark \
  --config Release
```

Run the resulting executable with, for example:

```bash
./build/sleep-benchmark/smu_precise_sleep_benchmark \
  --first-threshold-us 0 \
  --last-threshold-us 2000 \
  --repetitions 100 \
  --p99-error-us 2 \
  --output sleep-benchmark.csv
```

On multi-configuration generators, the executable is under the `Release`
subdirectory. Keep the machine idle during a run. The recommended threshold is
printed to the terminal; the CSV can be used to inspect each threshold using
absolute timing error (`abs(elapsed_us - target_us)`) and `cpu_us`.

Compile the native Linux target:

```bash
cmake -S . -B build/check -DCMAKE_BUILD_TYPE=Release
cmake --build build/check --target suspend --parallel
```

Release maintainers should also follow [releasing.md](releasing.md).
