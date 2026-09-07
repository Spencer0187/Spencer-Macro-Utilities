# Rust rewrite plan

The Rust rewrite is developed on `rewrite/rust`. The existing C++ application remains in this
branch until parity is proven and is treated as the behavioral oracle, not as an implementation
template.

## Non-negotiable compatibility targets

The replacement must preserve observable SMU behavior unless a deliberate compatibility change is
separately approved:

- Dear ImGui layout, styling, interaction, and script-generated controls.
- Existing `SMCSettings.json` and legacy settings behavior, including recovery/migration paths.
- Hotkey encoding and ordering.
- Built-in macro action order, timing, cancellation, and cleanup.
- Lua API semantics, resource limits, cancellation, and managed-input cleanup.
- Roblox process/log behavior.
- Platform capabilities on Windows, Linux X11/Wayland, and macOS.
- Updater and packaging behavior.

The rewrite may improve internals aggressively. Compatibility tests decide whether a change is safe.

## Migration order

1. Capture behavior from C++ as fixtures, traces, and UI screenshots.
2. Establish strict Rust workspace/tooling and compatibility utilities.
3. Prove SDL3 + Dear ImGui + OpenGL3 rendering in Rust with a representative UI slice.
4. Port deterministic foundations: key/hotkey types and profile/config compatibility.
5. Introduce process, Roblox, input, display/capture, and network services with fake backends.
6. Build the macro runtime against fakes and timing contracts before using native backends.
7. Port built-in macros one vertical slice at a time, including their UI and parity tests.
8. Port scripting after the underlying services are stable.
9. Replace the C++ entry point only after the compatibility matrix is green.

## Current Rust service slices

- `smu-process` owns process discovery/selection plus Linux, Windows, and a first native macOS process
  backend. The macOS backend mirrors the Objective-C++ `NSWorkspace` path: selectors are split on ASCII
  whitespace, decimal PID tokens are accepted only when `NSRunningApplication` can resolve them, named
  candidates compare normalized localized/bundle/executable/bundle-path names, Roblox player aliases exclude
  Studio, results are sorted/deduplicated and cached for 250 ms, `findMain` prefers a matching frontmost app,
  suspend/resume use `SIGSTOP`/`SIGCONT` while refusing the current process, and foreground checks compare the
  frontmost PID. AppKit objects stay inside autorelease pools and only plain Rust PID/cache data is retained.
  A dedicated `macos-latest` rewrite-CI lane tests and clippies the native macOS process/input/display/updater
  crates, the SDL-free macOS updater helper, and `smu-app`; those native adapters are now composed into the Rust application. The
  Windows backend mirrors C++ Toolhelp enumeration: the full selector is one executable name,
  matching is case-insensitive, `findAll` preserves snapshot order, `findMain` chooses the newest
  queryable creation time, suspend/resume dynamically resolves `NtSuspendProcess`/`NtResumeProcess`,
  and foreground checks compare the active window PID directly. Fedora validation covers the portable
  selection tests plus Windows-target Cargo metadata/API signatures; this machine does not have a
  Windows Rust stdlib, so native Windows `cargo check` remains an external validation step. Linux
  process control covers
  cgroup-v2 child freezing for safe app cgroups, ptrace-aware
  SIGSTOP refusal, process-tree SIGSTOP/SIGCONT fallback ordering, and the existing root
  `processhelper` socket protocol behind an injected authorization gate. Foreground detection keeps
  Wayland/unknown sessions in the C++ false path and, like C++, treats X11 as capable only when the
  `_NET_ACTIVE_WINDOW` / `_NET_WM_PID` atoms plus the root active-window property are available. That
  capability is cached once and shared by runtime gating and all built-in/imported-script foreground
  checkboxes; PID matching still reconciles namespace PIDs against target cgroups for Sober. `smu-scripting`
  wires the three legacy freeze globals through these capabilities with frozen-PID snapshots. On
  Linux, `smu-app` now also owns the C++-style two-minute authorization gate and Additional Freeze
  Permission Required modal; approval stages the bundled `processhelper`, launches it through
  graphical `pkexec`, verifies the root peer, and wakes the blocked process-control caller. The live
  Rust Freeze slice composes this service end-to-end. `smu-app` now shares one characterized target
  cache implementation across live macro controllers: process matches refresh at most once per
  second, X11 foreground eligibility at most every 25 ms, foreground matching always considers every
  matching PID, and process-control users may independently select main-vs-all targets.
- `smu-input` owns separate physical-state and injection contracts, C++-oracle-tested loose/strict
  hotkey matching, transient-input boundaries, and both native-layout and historical fallback text
  entry. A first dormant native macOS output adapter now mirrors the CoreGraphics injection half of the
  Objective-C++ backend: the full Windows-VK-to-macOS key table is differential-tested against
  `core/keymapping.h`; Accessibility trust is rechecked at each output site after explicit initialization;
  keyboard, mouse-button/X-button, relative/absolute cursor, and line-wheel events carry the same
  `0x534D554D41434F53` source-user-data tag and post at the HID event tap. Relative movement defaults to the
  legacy game-safe delta-field mode, with the historical visible-cursor movement preference exposed as an
  explicit setter, and direct absolute positioning reports the same Accessibility failure class. The macOS
  input slice now also owns a separate `MacosInputState`: it starts the legacy listen-only Session event tap
  on a named private thread, ignores SMU-tagged injected events, re-enables taps disabled by timeout/user input,
  mirrors the C++ mac-key/modifier/mouse-button state transitions, stores wheel direction only as transient
  binding input, exposes the same first-pressed-key scan, and clears state on deterministic shutdown. The
  callback owns only an `Arc` of atomic Rust state; CoreFoundation tap/run-loop objects stay on the reader
  thread, which runs in short default-mode slices so shutdown does not retain a cross-thread run-loop pointer.
  Native text entry now mirrors the Objective-C++ Text Input Services path too: it retains the current keyboard
  layout input source, requires a valid Unicode-layout `CFData`, scans all 128 macOS keycodes across none/Shift/
  Option/Shift+Option with `UCKeyTranslate` and dead-key generation disabled, posts Option before Shift and
  unwinds modifiers in reverse order, and falls back to tagged Unicode CoreGraphics key events when no active-
  layout physical key combination resolves the requested byte. The Carbon ABI shim is deliberately minimal and
  macOS-only; `CFData` lifetime is bounded by the retained TIS input source. These adapters are now composed into
  `smu-app` behind the macOS privacy-permission gate. Linux character
  injection queries the current X11 keyboard/modifier maps and mirrors the C++
  six-state `XLookupString` search (`none`, Shift, Mod1, Mod5, Shift+Mod1, Shift+Mod5), including the
  first-key-per-modifier and X11-keycode-minus-8 evdev rules; synthetic-map tests freeze plain Shift
  and Mod5/ISO-Level3 behavior. If native layout resolution is unavailable, callers retain the exact
  compatibility US key map. Cursor/screen/pixel APIs are intentionally excluded; those belong to
  `smu-display`.
- `smu-input` now has a native Linux evdev/uinput backend: it creates the historical `SMU Native
  Linux Input` virtual device, passively reads physical keyboard/mouse event devices, keeps injected
  events out of physical state, preserves wheel binding sentinels and generic modifier collapse,
  supports X11 absolute pointer warps, and falls back to the C++ 12-second interactive device
  discovery path when capability probes do not identify both keyboard and mouse. The full VK/evdev
  mapping is differential-tested from a generated C++ oracle fixture. On native Wayland, the Linux
  input backend can share `smu-display`'s approved RemoteDesktop session for direct `moveMouseAbs`
  while correctly continuing to report global cursor-position reads as unavailable.
- Linux `smu-input` now also exposes the C++ device-permission probe for `/dev/input/event*` and
  `/dev/uinput`, and `smu-app` composes the documented recovery flow end-to-end. The app owns one
  persistent `EvdevUinputInputDevice`; macro/script controllers are created once against that shared
  service even when startup permissions are missing, while a non-persistent shell gate disables all
  hotkey-driven execution until the backend is ready. The required modal shows the exact read/write
  probe status and can stage the bundled installer outside AppImage FUSE before graphical `pkexec`,
  launch the terminal sudo/su fallback, copy the manual sudo command, open `LINUX_SETUP.md`, or retry
  in-place. A successful retry initializes the same input service, so existing controller threads
  become live without restarting the application. Installer/launcher staging uses private temporary
  files and cleanup tests; unit tests never alter real device permissions or run the installer.
  Production Linux startup now also matches C++ by refusing to run the full GUI as root and directing
  elevation only to temporary helpers. Parity capture explicitly suppresses the permission modal and
  root refusal so screenshot tests remain host-permission-independent.
- `smu-display` owns monitor/cursor geometry, active-monitor refresh metadata, script coordinate
  modes, pixel colors/rectangle reads, and the screen-capture capability. macOS now has a dormant native
  CoreGraphics backend matching the Objective-C++ service: active displays come from `CGGetActiveDisplayList`,
  cursor-local monitor selection uses the same containment/nearest-display rule, monitor/global bounds preserve
  the legacy floor/ceil union behavior, refresh metadata comes from the active `CGDisplayMode`, and script
  permission hooks distinguish Accessibility from Screen Recording instead of silently reporting readiness.
  Pixel sampling preserves the 16 ms per-display `CGDisplayCreateImage` cache, maps logical display coordinates
  into backing-image pixels for scaled/Retina displays, isolates one pixel with `CGImageCreateWithImageInRect`,
  and renders it through a 1x1 device-RGB premultiplied-last/32-bit-big-endian bitmap context. Retained
  CoreGraphics objects own image/color/context lifetimes, and the deprecated synchronous display capture call is
  intentionally localized for the same macOS 10.15 compatibility as C++; ScreenCaptureKit remains a later
  deployment-target migration. The backend is now composed into the macOS app. Linux has native X11/XWayland
  cursor/Xinerama/root-window pixel support plus a native Wayland portal service using ScreenCast,
  RemoteDesktop, and PipeWire. The Wayland service keeps portal/PipeWire objects on an owned worker,
  converts BGRx/BGRA/RGBx/RGBA frames into immutable RGB snapshots, maps captured pixels back to the
  portal's logical monitor size for absolute pointer motion, and preserves the C++ activation-generation
  handoff: Lua workers wait while the app owns the approval modals. `smu-scripting` wires
  `moveMouse`, `moveMouseAbs`, `moveDegrees`, mouse motion mode, `getPixelColor`, `getPixelRect`, and
  the macOS cursor toggle through separate input/display services. `moveDegrees` caches its saved
  camfix/sensitivity calibration at API registration and is differential-tested against a C++
  oracle, including `strtod` hex-float acceptance and rounding/overflow errors.
- `smu-roblox` owns incremental Roblox player-log discovery/parser state plus a thread-safe log
  source capability. Its fixture suite is generated by the C++ `roblox-file` oracle, including state
  transitions, server metadata, numeric conversion quirks, truncation/rotation handling, and
  partial-line reads. `smu-scripting` exposes `readRobloxLog` / `robloxLog.read` plus ordered
  `robloxLog.onLine` callbacks and `listenUntilCancelled`; callback limits, reentrancy guards,
  cancellation, and a representative ECMAScript regex corpus are checked against the C++ path.
- `smu-updater` owns release/version contract validation, update-check orchestration, and
  cross-platform update-asset scoring. Linux has the production `curl` transport matching the C++ HTTPS-only
  redirect policy, 60-second timeout, 512 MiB limit, and GitHub updater headers. macOS now reuses that hardened
  transport with the legacy 15-second metadata / 120-second file timeout profile; `smu-app` now composes that
  manifest-driven check in its always-present macOS platform service, so update status/manual-release actions work
  even when Accessibility or Screen Recording permission keeps macro input disabled. A read-only macOS install-eligibility slice also resolves the
  enclosing `.app`, rejects mounted-DMG/symlink launches, requires `codesign --verify --deep --strict` plus a
  non-empty extracted leaf certificate, requires the app's containing folder to be owned by the current user,
  rejects group/world-writable mode bits and any macOS extended ACL on that folder (with the sticky-directory
  ownership rule retained as a further guard), and proves sibling staging writability with a create/remove probe. The next dormant slice now stages only manifest-authenticated
  ZIP bytes: a Rust validator requires one `.app` root, tolerates matching `__MACOSX`/AppleDouble metadata from
  `ditto --sequesterRsrc --keepParent`, rejects traversal/backslash, encrypted, duplicate, special-file, excessive-
  entry/expanded-size, outside-root, and incomplete app payloads, then writes those exact authenticated bytes to
  a private 0700 temp directory for `/usr/bin/ditto -x -k` extraction. The staged bundle must be a real directory,
  pass `codesign --verify --deep --strict`, and carry the exact same extracted leaf signing certificate bytes as
  the installed app; its staging tree is RAII-cleaned. This API remains dormant and never mutates the installed
  bundle. A further dormant transaction layer now copies that already-validated staged `.app` into a private
  same-filesystem swap directory beside the installed app, revalidates the signer after the copy, and records
  `(st_dev, st_ino)` identities for both old/new bundles before any live rename. Preparation also opens and holds
  the install-parent and private swap directories with `O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`; activation,
  rollback, commit, recovery, and recursive cleanup then use `fstatat(..., AT_SYMLINK_NOFOLLOW)`, `renameat`,
  `openat`, and `unlinkat` relative to those pinned descriptors rather than trusting mutable pathnames. The
  descriptor-relative cleanup never follows symlinks or crosses a device boundary; each root/child directory is
  opened with `O_NOFOLLOW` and its opened `(st_dev, st_ino)` is rechecked before recursion. Because macOS has no
  unlink-by-directory-handle primitive, cleanup intentionally retains the now-empty rollback/update root and
  hidden swap directories instead of reopening a final pathname-based `rmdir` TOCTOU. Activation moves the
  old app into rollback storage, renames the prepared new app into place, and returns an RAII transaction:
  explicit rollback or Drop restores the pinned old bundle, while commit first rechecks that the installed
  pathname still names the pinned new bundle and opens/verifies the pinned rollback directory object before
  disarming recovery and beginning destructive cleanup; identity/open failures return recoverable rollback state
  instead of destroying the backup. A same-UID rename/replacement of the hidden swap
  pathname therefore cannot redirect live mutation; regression
  tests rename that directory and install an attacker-controlled substitute while the transaction continues on
  the original pinned object. Post-mutation recovery is phaseful on both sides of activation: activation recovery
  records `OldMoved -> OldRestored -> PreparedUpdateRemoved`, while rollback records `NewActivated ->
  NewMovedAside -> OldRestored -> FailedUpdateRemoved`. Errors retain the boxed recovery state with the original
  pinned directory handles transferred out of the transaction (no `try_clone()`/new-FD allocation), so explicit
  `recover()` resumes from the exact completed phase instead of replaying a rename;
  Drop performs one best-effort recovery attempt. Regression tests cover resuming after the old app was already
  restored and after the new app was already moved aside. A dedicated SDL-free `smu-macos-updater-helper` is now
  the final dormant handoff layer: it runs only from a private detached directory owned by the current user,
  with no group/other mode access and no extended ACL, verifies that standalone helper against the installed
  app's exact leaf release certificate, registers the old
  PID with `kqueue(EVFILT_PROC/NOTE_EXIT)` and confirms `proc_pidpath` matches the installed SMU executable, then
  re-fetches the exact requested GitHub tag, checks helper-version compatibility, re-downloads the selected macOS
  asset through the hardened curl transport, and re-runs manifest ZIP/signer/version staging against the explicit
  installed bundle. Only after all of those network/trust/staging checks succeed does it write its PID to the
  inherited readiness-pipe capability supplied by the parent; the parent waits for that exact four-byte PID signal
  before allowing restart, so same-UID processes cannot spoof readiness with a filesystem entry and the old app
  cannot disappear before process pinning and staging are complete. Once the registered
  old process exits, the helper activates the descriptor-pinned transaction, clears quarantine, creates the
  legacy permission-repair marker, launches the updated `.app` with `/usr/bin/open -n`, and uses the phaseful
  rollback/recovery transaction if LaunchServices rejects the new app or if commit-time identity checks fail;
  non-recoverable commit errors are treated only as post-commit rollback-cleanup failures. Rewrite CI now also
  builds `smu-app`, the SDL-free helper, and Cargo-built SDL3 separately for `arm64` and `x86_64`, merges all three
  with `lipo`, stages the Rust-specific runtime asset layout into a real `.app`, signs it with an ephemeral
  certificate-backed test identity, verifies app/helper leaf-signer equality plus Mach-O/rpath/plist/runtime
  layout, and packages the exact `macos-universal` updater ZIP/manifest contract. That artifact is validation-only:
  the legacy signed production `build.yml` remains untouched. `smu-updater` now also contains the dormant
  parent-side handoff primitive: it requires a canonical target, revalidates the installed `.app`, opens/pins the
  embedded helper, proves its leaf signer, copies the exact helper bytes into a fresh current-user 0700/no-ACL
  temp child, rehashes and revalidates the detached helper signer/identity, creates a private inherited readiness
  pipe, and launches the exact `--apply-update <version> <bundle> <old-pid> <ready-fd>` contract. The pipe read end
  is close-on-exec, only the helper inherits the write end, and the parent accepts exactly the spawned helper PID
  encoded as four native-endian bytes after all network/trust/staging work completes; it then requires the helper
  to remain alive through a short handoff-survival window before reporting success. Helper creation/failure cleanup
  is `openat`/`unlinkat` relative to a pinned launch-directory descriptor, and the launch-directory pathname is
  rechecked against its pinned `(st_dev, st_ino)` before signer validation and process spawn. The child independently
  requires the exact `SMU-Update-Helper` executable name and `smu-update-launch-<old-pid>-...` directory directly
  under canonical macOS temp, pins that same directory object through signer validation, verifies the inherited FD
  is a live write-only FIFO, and only then uses it as the handoff capability. Failed/timeout handshakes terminate
  the child before returning; the child removes its detached executable through the pinned directory FD on exit.
  The now-empty private root is intentionally retained because macOS has no safe unlink-by-directory-handle primitive.
  The remaining same-UID race is the final pathname resolution performed by macOS process spawn between the last
  detached-file identity check and `exec`; no descriptor-exec primitive is available in the targeted libc surface,
  so this residual is documented rather than overstated as eliminated. `smu-app` now composes this launcher behind
  the updater apply worker and normal restart exit path, but `check_for_updates` still constructs the macOS checker
  with `auto_apply=false`, `request_apply`/`confirm_apply` both recheck that capability, and the Settings UI therefore
  cannot enter the launcher in production. The remaining cutover is intentionally the production Rust `.app`
  signing/publishing trust decision plus an explicit flip of that auto-apply capability.
  HTTP remains injected behind `UpdateTransport`, and platform replacement remains separate, so untrusted
  metadata is validated before install-side effects are possible.
- `smu-app` now composes the live Linux imported-script runtime and UI over `smu-scripting`.
  `ScriptManager` remains the sole owner of Lua instances, cancellation, and named per-record workers;
  the app controller owns only global macro enable, target-foreground eligibility, and C++-compatible
  per-script hotkey latches. A running script's rising edge force-stops before the foreground gate,
  while an idle foreground-rejected script resets its latch so a held key becomes a fresh edge after
  focus returns. The sidebar and selected-script pane now support hotkey/enable/foreground edits,
  Execute Now, Reload, Remove, Force Stop, Revert Module, metadata/status/error display, and captured
  `onSettings()` controls. Running scripts use cached read-only settings semantics: interactive
  controls are disabled while text/separators remain visible and `dynamicTextbox` stays selectable and
  copyable. Imports use SDL3's native file dialog for `.smus/.hss/.lua/.txt`, suppress all macro
  hotkeys while the external modal owns input, and now fall back to an in-app Dear ImGui filesystem
  browser when SDL reports the native picker unavailable. The fallback starts at the current directory,
  lists directories before supported script files, supports double-click navigation/open plus an editable
  relative/absolute path field, and keeps macro input captured until it closes. Selected files then enter
  the same explicit trust modal with a read-only source preview before manager import. C++ uses the
  separately compiled ImGuiFileDialog widget for this rare fallback path, so Rust validates its fallback
  behavior with filesystem-model and headless-render tests rather than unsafe cross-version Dear ImGui
  ABI sharing. Imported-script persistence is now attached to the live profile
  adapter: a successful import immediately saves the current profile like C++ and reports
  `Script imported, but the settings file could not be updated.` if that write fails; ordinary profile
  saves serialize the manager records, profile loads deserialize the selected record set, and
  `savedSetting`/`getSavedValue` reads from a thread-safe normalized snapshot of the currently
  loaded profile. The default parity case remains script-free and storage-isolated, and its pixel gate
  is unchanged.
- `smu-app` now owns C++-compatible `Revert Module` behavior for all 16 built-in sections and for
  imported scripts. A new C++ oracle command generates `compat/profiles/reset_section_cases.json`;
  Rust applies each adversarial sparse payload, resets one section, overlays back onto the full default
  preservation baseline, and compares the complete serializer snapshot for all 16 sections. This
  freezes section-local/global reset side effects such as Item Unequip's Enter key and Bunnyhop's
  shared chat override, while preserving unrelated settings. Sections 5/6/11 intentionally mirror the
  C++ multi-instance quirk: only instance 0's module data resets, and its serialized base enable/target
  flags remain instance-owned. The differential fixture also exposed and fixed a pre-existing profile
  omission where Rust dropped per-Wallhop-instance `wallhopcamfix`; base and extra instances now
  round-trip that compatibility field even though live wallhop execution still uses the separate global
  sensitivity/camera-fix calibration path.
- `smu-app` now owns preservation-safe live profile startup, Save To, Load, duplication, rename,
  deletion, selection autosave, shutdown save, window geometry, imported-script persistence, and Lua
  saved-setting integration over `smu-profile`. `PreservedProfilePayload` keeps unknown/future keys and
  real script arrays while exposing C++-normalized values for live state; app saves batch-overlay the
  Rust-owned settings onto that preservation baseline so untouched data survives round trips. Startup
  mirrors C++ missing-file creation, legacy-flat conversion into `Profile 1`, preferred-profile load,
  immutable `(default)` refresh, `(default)` promotion, and unreadable-source refusal. Legacy
  conversion materializes the complete known serializer surface but intentionally drops unknown legacy
  root keys, matching C++ deserialization/serialization. The live `ShellState` adapter is checked
  against every generated C++ payload-oracle case: all 19 one-shot apply cases and all 6 stateful
  sequences match the full serializer snapshot, including fixed-buffer truncation, numeric/buffer
  precedence, prefix arrays, section-order upgrades, multi-instance rebuilds, and historical aliases.
  Startup and filesystem tests use temporary files only; parity sessions are pathless. Real user
  settings are never used as a test target.
- `smu-app` now owns the C++ Theme Editor and theme metadata path. Rust carries the six built-in
  presets plus the editable custom theme, applies the selected definition to the ImGui context, and
  routes manual/status colors through that same definition for the global strip, macro panes,
  multi-instance/sidebar rows, imported scripts, Settings/updater/Wayland status, Lag Switch, and the
  Rust-only backup warning. The main SMU window still forces zero rounding because C++ explicitly does
  so; floating windows use the selected theme rounding values. Profile metadata preserves and updates
  `current_theme_index`, `custom_theme`, named `themes`, saved-but-not-loaded `show_theme_menu`, and
  `shortdescriptions` in the same preservation-safe mutation as the live profile payload. Numeric theme
  indices follow C++ integer conversion/fallback behavior, theme floats use the C++ three-decimal
  rounding expression, unknown theme entries append by name, and invalid indices fall back to
  Cyberpunk. The default Cyberpunk capture remains pixel-identical at the compatibility gate.
- `smu-app` now owns the first live Linux Settings-window slice and shared keybind editor. The
  floating window is opened by the existing global Settings button and is backed by real app/service
  state for process-found status, Shift/Chat keybinds, chat override, paste delay, global zoom/reverse,
  display-scale/profile values, sidebar-description suppression, and native Wayland ScreenCast /
  RemoteDesktop status and actions. The floating Settings and Theme Editor windows now clamp their
  appearing and live positions to the main viewport with the same bounds math as C++, including offset
  viewports and windows larger than the viewport. The RemoteDesktop enable button is hidden once control is active,
  matching C++, and the sponsor text uses the hand cursor/click interaction to open the Spencer0187
  GitHub Sponsors page through SDL. The always-visible global header now also matches the C++ link/
  help interactions: the official GitHub source uses the hand cursor and SDL URL opening, hovering
  the Anti-AFK label exposes the same Windows-only behavior tooltip without changing header geometry,
  and the Settings/Theme Editor buttons flip from `->` to `<-` while their windows are open.
  Wayland starts/stops run on one owned Settings worker so portal
  prompts and first-frame waits never block the UI thread; process status is cached for one second.
  The keybind editor preserves the C++ release-before-capture flow, modifier packing/normalization,
  wheel transient capture, modifier-only release binding, and post-bind hotkey suppression; all live
  macro controllers receive the resulting effective-macro gate, and Escape remains bindable while
  capture is active. Every built-in pane now uses that same live binder for its main trigger, and the
  additional C++-editable bindings are cut over too: HHJ auto keys 1/2, Item Unequip's shared Enter
  key, Press a Button output keys, Wallhop jump keys, and Spam a Key output keys. Multi-instance
  binders keep per-instance IDs so capture state follows the stored key rather than the visible pane.
  Linux `smu-input` now forwards current-key/transient methods through `KeyState`,
  fixing trait-object wheel capture. The Settings window also now composes the live Linux updater
  check and AppImage auto-install path: the global header starts one background check at startup,
  exposes C++'s `(OUTDATED VERSION)` marker after a successful newer-release check, and Settings keeps
  explicit retry/check controls. Release metadata/manifest validation runs through `smu-updater`, and SDL
  opens the official release page for manual installation. Auto-apply is advertised only when the
  current process is actually running from the declared AppImage mount and the external AppImage is
  safely replaceable. Downloads remain HTTPS-only and are rechecked against manifest size/SHA-256;
  direct images and exact root-level AppImages extracted from Linux ZIP bundles must pass the current
  architecture's 64-bit little-endian ELF header check before staging. Replaceability preserves the
  APPIMAGE/APPDIR, strict mount-descendant, sticky-directory ownership, and same-directory write-probe
  guards. A detached helper waits for normal Rust profile/service shutdown, swaps through a unique
  backup, relaunches the new image, and restores/relaunches the previous image if installation or the
  two-second startup check fails. The C++ Yes/No/Cancel/manual-install confirmation flow is preserved,
  while a successful Rust staging worker requests clean app shutdown instead of exiting from that
  worker. Installer tests use temporary files/ZIPs only and never invoke the live helper or current
  APPIMAGE. Debian/RPM/portable/Nix installs remain manual. Update work is disabled during parity
  capture. Anti-AFK is Windows-only in the legacy runtime and has no Linux execution path to port;
  the non-Windows global header therefore forces its persisted toggle false each frame like C++.
- Linux AppImage, Debian/RPM, and portable-folder packaging now ship the Rust `smu-app` entry point
  instead of the C++ compatibility executable. The default Linux CMake `all` target now builds/stages
  Rust and does not create the legacy C++ GUI target at all. `SMU_BUILD_CPP_LINUX_COMPAT_APP=ON`
  explicitly adds that compatibility oracle alongside Rust, while `SMU_PACKAGE_RUST_LINUX=OFF` remains
  the legacy C++ packaging escape hatch. `lua_static` and `smu_platform_linux` are `EXCLUDE_FROM_ALL`
  compatibility libraries and are pulled in only by C++ GUI/tests/oracles that need them. A dedicated
  staging helper performs a locked release Cargo build, copies the exact SDL3 shared library produced
  by that Cargo build, and links the app with `$ORIGIN/lib` so the portable tree is
  self-contained for SDL. Runtime font lookup prefers assets next to the executable and then AppDir
  locations before the source-tree fallback, so packaged Rust builds no longer depend on the checkout
  layout. The Rust Wayland pointer path uses the desktop RemoteDesktop portal directly, so release
  metadata and AppImage assembly no longer declare or bundle `libei`. The Nix derivation now builds
  `smu-app` with `rustPlatform.buildRustPackage`, imports the committed Cargo lockfile for offline
  dependency vendoring, runs both Go helper test/build steps without network access, stages the exact
  Cargo-built SDL3 shared library, and installs both `nethelper` and `processhelper` inside `SMU_APPDIR`.
  The flake/package surface therefore uses the Rust entry point too; this checkout has no local `nix`
  executable, so the derivation is structurally guarded and host-layout/runtime-smoked here, while a
  real `nix build`/`nix flake check` remains the external validation step.
- `smu-app` now owns the live Linux Item Unequip COM Offset and Laugh Clip slices, completing live
  Linux pane/controller cutover for all 16 built-in macro sections. Both use one shared macro-text
  path: `OpenChatForMacro` first tries native-layout `/` only when chat override is enabled and falls
  back to the live configured chat hotkey when that native attempt fails; normal text tries native
  layout per byte and then the historical fallback map. Item Unequip keeps the C++ single-worker active
  guard, captures only custom/emote text before spawn, re-reads paste/Enter/dropdown/slot/keep-equipped
  settings inside the worker, and preserves the custom-text early return plus three separate dropdown
  reads. Laugh Clip keeps the same active guard and re-reads zoom mode, shift key, and S-movement state
  independently at action and release sites, so mid-worker setting drift retains the characterized
  asymmetries. Linux built-in execution, settings UI, and production entry-point cutover are complete.
  The first cross-platform composition prerequisites are now in place too: `smu-core`, `smu-input`,
  `smu-process`, and `smu-scripting` are ordinary `smu-app` dependencies instead of Linux-only
  dependencies; a
  platform-neutral `ScriptProfileBridge` owns imported-script serialization plus the live saved-setting
  snapshot; and `ScriptAppServices` now owns script UI snapshot preparation, native-dialog/fallback
  handoff, import/settings mutations, process-target propagation, and script shutdown around an injected
  `ScriptManager`. Linux delegates its existing script/profile behavior through those shared services
  without changing runtime behavior. The generic app loop now also owns `ScriptAppServices` on non-Linux:
  imported-script sidebar/detail/footer UI, native-dialog/fallback import, profile persistence, saved-value
  reads, process-target propagation, and script/settings mutations are all available there through a
  `ScriptManager` constructed with `ScriptServices::default()` plus the saved-setting source. Platform API
  calls that require native backends therefore remain unavailable until composition attaches them. macOS now
  replaces that fallback with a dedicated composition root: native process discovery/control and display are
  always attached to imported-script services, while native input/state and the shared script-hotkey runtime
  start only when both Accessibility and Screen Recording are already trusted. The same composed input facade
  feeds the built-in keybind editor, and shutdown stops script runtime before the event-tap/injector pair. The
  shared controller model modules compile on macOS because the cross-platform UI/profile layer references their
  settings types. With native input ready, macOS now starts the same 15 shared built-in controllers exercised by
  the C++ `MacroRuntime`—Freeze, Item Desync, Press Key, Wallhop, Walless LHJ, Speedglitch, HHJ, Floor Bounce,
  Item Unequip, Item Clip, Laugh Clip, Wall Walk, Spam Key, Ledge Bounce, and Bunnyhop—and updates their settings
  from the live shell each frame. Anti-AFK remains omitted because the C++ implementation is Windows-only, while
  Lag Switch remains unavailable because the macOS network backend is null. macOS update checking, settings status,
  manual release-page actions, updater shutdown, and the guarded restart handoff are now composed in the app-level
  platform service independently of macro-input readiness. The signed replacement launcher is wired into the apply
  worker, but automatic installation remains unreachable because the macOS checker still advertises
  `auto_apply=false`; production Rust `.app` signing/publishing must replace the legacy package path before that
  capability is deliberately enabled. Windows now injects `smu-process::windows` into the portable `ScriptServices`, so imported scripts can
  discover matching processes and suspend/resume them with the native backend. A platform-neutral
  `ProcessStatusService` now owns the selector-sensitive one-second process-found cache too: Linux uses
  its existing `ProcDiscovery` plus parity override, while Windows uses `WindowsProcessBackend`, so the
  global process-status UI is native on both platforms. Imported-script hotkey runtime control is now
  native on Windows too: `WindowsKeyState` reads physical Win32 key/button state with
  `GetAsyncKeyState`, the shared `ScriptRuntimeController` depends only on `KeyState`, and foreground
  gating uses `WindowsProcessBackend`. That same Win32 device now implements `InputInjector` with
  tagged `SendInput` scan-code keyboard events, mouse-button events, wheel input, current-key scans, and
  relative mouse motion, and Windows `ScriptServices.input` is populated with it. Relative motion uses
  the live C++ `display_scale` rule while `move_mouse_raw` stays unscaled, and tagged injections honor
  the C++ foreground-GUI budget of 50 events per 16 ms using the SDL-provided HWND. `WindowsKeyState`
  now also implements direct absolute positioning with the C++ virtual-desktop normalization rule, and
  `smu-display::windows::WindowsDisplayCapture` supplies cursor position, active-monitor bounds/refresh
  metadata, plus the legacy monitor-local cached DIB/`BitBlt` pixel sampler. Windows
  `ScriptServices.display` is populated with that backend, so imported-script `moveMouseAbs`,
  `getPixelColor`, and `getPixelRect` now use native Win32 display services alongside `pressKey`,
  `clickMouse`, managed hold/release/state, `isKeyPressed`, `mouseWheel`, and relative `moveMouse`.
  Physical wheel bindings are native too: a reference-counted `WH_MOUSE_LL` hook is shared by both
  `WindowsKeyState` owners, ignores Win32/SMU-tagged injected events, stores the latest wheel direction
  in the same single transient slot as C++, and shuts down when the last input owner drops. Windows
  network migration has started at the deterministic boundary: `smu-network` now builds the exact C++
  WinDivert direction/protocol/Roblox/custom filter grammar, including process UDP-port precedence and
  dynamic-IP set ordering. The shared `LagSwitchControl` contract now also owns backend initialization,
  base config, and base blocking, so `LagSwitchController` no longer depends on the Linux concrete
  backend and can drive a Windows transport through the same runtime path. `smu-network::windows` now
  provides the native loader/ABI foundation too: it requires `SMCWinDivert.dll` and
  `SMCWinDivert64.sys` beside the executable, resolves `Open`/`Recv`/`Send`/`HelperParsePacket`/
  `Shutdown`/`Close`, models the bundled 80-byte `WINDIVERT_ADDRESS`, bounds-checks the parser-returned
  payload pointer before exposing it as a Rust slice, and probes driver availability with the C++
  `Open("false", NETWORK, 0, SNIFF)` call. This Fedora toolchain has neither a Windows stdlib nor
  `rust-src`, so the Windows-only loader is validated here with rustfmt, locked `windows-sys` API
  signatures, Windows-target metadata, source guards, and the bundled DLL export table; native Windows
  `cargo check` remains external. The packet-decision core is now frozen in portable Rust as well:
  hard-drop direction, the 9.5s/19.9s/290s prevent-disconnect pulse intervals, RCC pulse behavior, the
  exact RakNet smart-drop byte signature, fake-lag direction/delay selection, and the C++ safety-clock
  reset quirk all have host-executable tests. The fake-lag sender's FIFO/deadline and shutdown semantics
  are frozen in a host-tested queue worker too, including C++'s front-of-deque behavior when a later
  packet has an earlier deadline. That queue worker now compiles on Windows as a crate-internal
  production primitive with `start`/`enqueue`/`stop`, and live WinDivert handle ownership is wired for
  delayed reinjection. A Windows packet worker now owns the real capture loop too: it opens the frozen
  filter, receives packets, applies the host-tested C++ decision core, immediately sends/drops or queues
  delayed packets, and shuts down the current handle to wake a blocked `Recv` before reopening on filter
  changes. Final filter validation and handle publication share the capture lock so an update cannot miss
  a just-opened stale handle, while delayed sends hold the handle-slot lock across `WinDivertSend` just as
  C++ holds `g_windivert_handle_mutex` across delayed reinjection. The delayed sender keeps one
  lifetime-long queue and resolves the current handle only when a packet becomes due, matching C++
  across restarts.
  `WindowsLagSwitchBackend` now owns the live `LagSwitchControl` state/arbitration layer too: it
  initializes/probes WinDivert, starts the packet worker, tracks base/script policy state, refreshes
  target-process UDP4/UDP6 owner ports and Roblox log-derived dynamic IP/RCC state on a one-second
  worker, and rebuilds the live capture when those targets change. The process-port path uses the
  shared Windows process backend and preserves C++'s main/all-PID selection, network-order port swap,
  sorted/deduplicated port set, and Roblox-target precedence. WinDivert `ERROR_INVALID_PARAMETER`
  recovery also mirrors C++ by dropping sticky dynamic Roblox IPs before retrying the filter. `smu-app`
  now composes that backend on Windows too: one shared backend/log reader feeds both the built-in 2 ms
  Lag Switch controller and imported-script `ScriptServices`, process-target changes propagate into the
  backend, the Windows UI owns manual initialize/shutdown plus status/warnings, and app shutdown stops
  the controller before tearing down WinDivert. Windows initialization now also preserves the actual
  C++ side effect: a Windows-only `LagSwitchControl` adapter publishes successful controller/script
  initialization into the compatibility enabled flag, while the shared controller itself remains free
  of that side effect so Linux keeps its existing helper/flag split. The scripting API now distinguishes
  platform support from initialized transport explicitly and cold-initializes before `lagSwitch(true)`
  takes script blocking ownership; a failed cold init leaves the script running without claiming the
  block, matching C++. Initialization failures now carry explicit `RequiresAdministrator`,
  `RequiresHelper`, or backend-error diagnostics. Windows checks effective-token administrator
  membership with the same SID/membership API family as C++, Linux reports the helper requirement, and
  the scripting execution context publishes the matching warning/error onto each script instance without
  throwing into Lua. Windows app composition now also owns the migrated built-in macro runtime rather
  than leaving those controllers Linux-only: one shared `WindowsKeyState`, `WindowsProcessBackend`, and
  `WindowsDisplayCapture` service set feeds process status, imported-script services, script hotkeys,
  WinDivert targeting, keybind capture, and the Bunnyhop/HHJ/Floor Bounce/Freeze/Item Desync/Item Clip/
  Item Unequip/Laugh Clip/Press Key/Spam Key/Ledge Bounce/Speedglitch/Wall Walk/Walless LHJ/Wallhop
  controllers. Their settings are refreshed from the shell every frame and every controller is stopped
  explicitly at app shutdown. Lag Switch remains its separately owned Windows controller because it also
  owns WinDivert lifecycle. Windows Anti-AFK is now composed as its own controller too: the existing
  low-level input-hook lease also counts real (non-injected) keyboard activity, the shared process backend
  selects the newest visible target window for background activation, and the controller reuses the
  characterized timer/key/click/focus timing while keeping the blocking activation sequence on one owned
  worker. It deliberately ignores the global Macro Toggle like C++, defaults on for fresh Windows
  settings, and remains suppressed through the keybind capture/release window. The shared low-level hook
  tracks whether the keyboard hook actually installed, and the shell emits a one-time warning if Anti-AFK
  is enabled without that real-keyboard activity source. An isolated Windows Rust staging path now exists
  without touching the signed C++ release:
  `scripts/stage_rust_windows_app.py` performs a locked Cargo build and creates a clean
  experimental directory containing a staged `Spencer-Macro-Utilities.exe`, Cargo's exact `SDL3.dll`,
  the full file-based `assets/` tree, and repository `SMCWinDivert.dll` / `SMCWinDivert64.sys` siblings.
  The Rust binary itself now uses the Windows GUI subsystem like the legacy `WIN32` CMake target, and the
  staging helper parses the PE optional header to reject an accidental console-subsystem build. Cargo's
  internal binary target remains `smu-app.exe` so Linux naming/staging stays unchanged. Windows UAC
  relaunch is now owned by Rust as well: failed WinDivert initialization requests the existing
  `Administrator Required` flow, suppression auto-queues the relaunch path, profile/script state is saved
  before relaunch, and a Windows-only `ShellExecuteExW(..., "runas", ...)` helper starts the current
  executable elevated while the original instance exits through normal controller/backend shutdown. The
  rewrite CI has a separate `windows-latest` job that runs Windows unit tests plus clippy, exercises the
  staging path, verifies the required siblings, and uploads only explicitly unsigned validation
  artifacts. It now also packages that staged tree as a future release-style ZIP rooted at
  `Spencer-Macro-Utilities/`, including all runtime siblings/assets plus the same release notices/licenses
  as the legacy Windows bundle. `generate_update_manifest.py` keeps the production asset mapping unchanged
  by default but supports an opt-in Windows asset override/platform-only mode so rewrite CI can emit and
  SHA-256-verify a Windows-only manifest against the Rust bundle without changing `build.yml` or the
  signed C++ release. The Rust PE now also embeds the legacy Windows product identity through a build script: the
  canonical repository `version` file drives numeric/string file and product versions, `smu.ico` is
  embedded as icon resource 102, and company/description/internal/original filename/product/copyright
  fields mirror the C++ resource script. Windows CI inspects the staged executable's version metadata so
  resource compilation is validated on a real Windows host. Embedded legacy RCDATA is intentionally not
  reused because the Rust runtime owns file-based assets/WinDivert siblings. Windows update checking is
  now native Rust too: `smu-updater` has a WinINet transport with the C++ updater headers, HTTPS-only
  policy, 10-second connect/receive timeouts, success-status validation, and the shared 512 MiB response
  limit; the Windows app feeds that transport into the same Settings/update-modal service used on Linux.
  Automatic Windows apply deliberately remains disabled while the trusted multi-file replacement path is
  completed. The unsigned multi-file bundle shape is frozen in rewrite CI, and `smu-updater` owns a
  platform-neutral Windows bundle validator/extractor that requires the single
  `Spencer-Macro-Utilities/` root, rejects traversal/backslash/drive-style and duplicate paths,
  encrypted/non-regular entries, excessive entry counts or expanded payloads, unexpected top-level files,
  and executable/script-bearing additions outside the frozen runtime set; only image/font assets and text
  license extensions remain intentionally extensible. It also verifies the required app/update-helper/SDL/
  WinDivert/font runtime siblings before retaining a staging directory. Extraction now creates the staging
  root atomically and refuses an already-existing destination instead of deleting or adopting it, so a
  staging-path race fails closed without touching attacker- or user-created contents. The Windows updater
  also now owns the C++ Authenticode trust primitives: WinTrust validates each executable's chain and
  revocation state, Crypt32 extracts the signer certificate SHA-256 fingerprint, and the public
  same-signer operation first verifies both executables before comparing their signer fingerprints.
  The updater also now owns a platform-neutral multi-file replacement transaction for the validated staged
  tree. It copies each managed file through a unique same-directory temporary file, moves overwritten files
  into a fresh rollback directory inside the installation root, rejects symlink/non-regular targets and
  overlapping staging/install roots, preserves user files outside the frozen bundle layout, and can commit
  or reverse the complete transaction; unit tests cover successful commit, explicit rollback, and automatic
  rollback after a mid-update filesystem collision. `smu-updater` now also exposes one shared downloaded-
  asset verifier: it enforces the manifest-declared size and SHA-256 digest and can return the exact
  authenticated bytes read from disk. The Linux updater has been switched to that verifier. `UpdateChecker`
  can re-fetch one exact canonical GitHub release tag and requires the returned release version to match the
  requested version, avoiding a race where `releases/latest` changes between confirmation and helper startup.
  The Windows updater can also read and strictly validate a PE's numeric `FILEVERSION` from
  `VS_FIXEDFILEINFO`, providing a caller-independent downgrade guard against the installed executable.
  A dedicated `smu-windows-updater-helper` binary now composes those primitives without SDL or the main GUI
  dependency chain. Rewrite CI builds and stages it as `SMU-Update-Helper.exe`, embeds matching Windows
  product/version resources, and includes it in the experimental release ZIP. Its dormant apply path
  re-fetches the exact requested release tag, re-verifies the caller-downloaded ZIP against the official
  manifest size/SHA-256, derives one SHA-256 per validated ZIP entry from those authenticated bytes,
  extracts only into a fresh staging directory, opens every staged file once and verifies it against the
  authenticated per-file digest set, and on Windows keeps those read handles open without write/delete
  sharing through signer/version validation, the old-process wait, and replacement. The copy path hashes
  the bytes read from those same handles again before activation, so staged pathname swaps or post-validation
  mutations fail closed. It also requires the running helper plus staged app/helper to share the installed
  app's valid Authenticode signer, enforces staged PE `FILEVERSION` equality with the target plus a strict
  installed-version upgrade, waits for the old process, performs the transactional multi-file replacement,
  and rolls back if relaunch fails or exits during the startup check.
  The main staged app is deliberately not used as that helper because its pre-main SDL DLL load would widen
  the trust boundary before bundle revalidation. The helper also refuses to run from inside the installation
  directory, making the future launcher responsible for copying the installed signed helper to a detached
  location before replacement so Windows never has to overwrite the running helper image in place. The
  replacement transaction now rejects symlink/reparse-point target ancestors and, on Windows, opens the
  install/rollback directory chain with `FILE_FLAG_OPEN_REPARSE_POINT` while allowing only read sharing; those
  handles remain live through activation and rollback restoration so writable/delete-sharing handles cannot
  swap junction/path identity during mutation. Existing runtime files are opened with
  `FILE_FLAG_OPEN_REPARSE_POINT`, validated and hashed from that exact handle, then on Windows moved into
  rollback storage with `SetFileInformationByHandle(FileRenameInfo)` so the object inspected is the object
  renamed. Rollback reconstructs from that pinned handle while rechecking its digest instead of trusting the
  backup pathname. The helper also opens the old PID before
  network/staging work with synchronization plus query rights, verifies that handle's
  image path is the installed SMU executable, and keeps the handle alive through the wait so PID reuse cannot
  redirect synchronization to an unrelated process. `smu-updater` also now has the dormant parent-side handoff
  primitive that a future enabled UI path can call: it derives the local updater version from the running
  executable's numeric PE `FILEVERSION`, re-fetches the exact release, downloads and validates the
  manifest-bound Windows package, creates and pins a fresh detached temp directory, copies the installed
  helper from a pinned non-reparse handle, requires the copied bytes, Authenticode signer, and PE version to
  match the installed helper/current app, pins the detached helper against mutation, and launches it with the
  exact target/package/install-root/current-PID arguments. The helper accepts the package only when it is a
  regular non-reparse sibling in that detached launch directory. On startup it first proves its own canonical
  helper name and `smu-update-launch-<old-pid>-...` directory under the Windows temp root, establishes cleanup
  ownership before validating the install/package paths, and deletes the ZIP immediately after its
  authenticated bytes are loaded. On every later exit path it launches the canonical System32 `cmd.exe`
  without a console to retry deleting only the detached package/helper and then remove the now-empty launch
  directory; reboot-delayed deletion remains a best-effort fallback when available. Helper failures are also
  surfaced through a native Windows error dialog and return a nonzero process exit status, avoiding silent
  failure from the GUI-subsystem helper. `smu-app` now composes the launcher in its Windows apply worker and normal
  restart exit path, but both `request_apply` and the Windows `confirm_apply` worker recheck the updater capability
  and the Windows checker still advertises `auto_apply=false`, so production UI cannot enter that path. Windows
  cutover has two additional hard gates beyond compiled wiring: production signing/publishing must establish and
  sign the helper/bundle-wide trust policy (the side-loaded runtime siblings include the repository
  `SMCWinDivert.dll`, which is currently unsigned), and the parent/helper handoff must gain a readiness barrier so
  the app does not exit until the helper has pinned/validated the old process. The signed legacy C++ `build.yml` /
  SignPath release path remains untouched until those gates are deliberately promoted. macOS now composes its
  native Rust process, input, and display adapters into the app for process status, imported scripts, script
  hotkeys, display APIs, built-in keybind capture, and the 15 shared built-in macro controllers when both privacy
  permissions are ready. macOS Anti-AFK remains absent to match the Windows-only C++ implementation, while
  packet interception/Lag Switch remains intentionally unavailable. macOS update checks are now wired through the
  always-present platform service (including Settings/manual release actions even without macro permissions), while
  the signed-bundle staging/swap transaction plus detached old-process wait/relaunch helper are now composed behind
  the guarded apply/restart state machine. Rewrite CI produces a certificate-backed signed universal Rust `.app`/ZIP
  validation artifact, but production `build.yml` still uses the legacy C++ macOS package path and the Rust macOS
  checker still advertises `auto_apply=false`. Those are the two explicit cutover switches; neither changes until
  production Rust signing/publishing is intentionally promoted and reviewed.
- `smu-app` now owns the live Linux Wall Helicopter High Jump slice plus its dedicated motion lane.
  The XButton1 rising-edge controller snapshots only the target PID vector; automatic-key, freeze,
  zoom/shift, HHJ length, and release settings remain live at the exact C++ worker read sites. The
  one-shot worker preserves the freeze-delay override/fast-HHJ branches and the historical quirk where
  HHJ length can activate the shared `isHHJ` state and later change to zero before the finish check,
  leaving that state active. A long-lived motion worker replaces repeated worker creation while keeping
  the section-2 + `isHHJ` gate and per-cycle FPS/strength live reads; X strength is read before the first
  move, Y strength only after the first frame wait. Linux Floor Bounce shares that same atomic rather
  than maintaining separate helicopter state.
- `smu-app` now owns the live Linux Floor Bounce slice. Its F4 rising edge snapshots the current target
  PID vector and launches overlapping one-shot workers. The fixed prefix remains direct Space-down,
  521 ms, suspend 72 ms, resume 72 ms, suspend 72 ms, resume, Space-up. Only after that 737 ms prefix
  does the worker re-read the experimental HHJ toggle; when enabled it re-reads delay 1, the shift key
  for bound key-down, delay 2, delay 3, then—after disabling shared HHJ state and a fixed 100 ms wait—
  re-reads the shift key for bound release. Negative configurable delays clamp to zero while retaining
  the wait boundary, matching the characterized C++ helper.
- `smu-app` now owns the live Linux Bunnyhop slice. Its 2 ms controller owns target-foreground
  eligibility, Intelligent Auto-Toggle chat locking, and the physical hotkey decision; the lock uses
  the C++ update order where chat-open is applied before Enter/left-click clears it, and guard exits do
  not reset it. Linux `smu-input` reads physical state only from real evdev devices and excludes SMU's
  uinput device, so the C++ keyboard-latch requirement is satisfied without a second synthetic state
  source. One long-lived owned worker replaces repeated activation-thread creation while preserving
  the active handoff: each iteration snapshots output hotkey and half-delay, performs bound key-down,
  waits the first half, always releases that snapshotted hotkey, and only then honors mid-cycle
  cancellation before the optional second half-delay. Wheel triggers remain physically ineligible and
  mouse-button/packed-modifier triggers use the characterized source classification.
- `smu-app` now owns the live Linux Ledge Bounce slice. The C-key controller keeps the C++
  rising-edge latch and foreground gate, then launches overlapping one-shot workers with no active
  guard. Sensitivity, Cam-Fix, and side are read at worker start; the worker executes the frozen
  90/40/16 ms prefix, re-reads horizontal realign at 146 ms, and re-reads auto-hold at 146 ms without
  realign or 216 ms with realign. The C-style sensitivity parser and turn calculation continue to
  come from the characterized `smu-runtime` helper, including invalid/negative/NaN flooring and
  overflow/truncation behavior.
- `smu-app` now owns the live Linux Spam a Key (`SpamKey`) slice as the third consumer of the
  shared multi-instance container. It preserves the constructor quirk where the UI initially displays
  `20` ms but the worker's `real_delay` remains `1000` ms until that text field is edited; edits use
  C++ `std::stof` numeric-prefix behavior and `int((delay + 0.5f) / 2)` conversion. Per-index runtime
  slots are stable and own one long-lived worker each. This intentionally removes C++'s dangling
  reference hazard while retaining copy-left/pop-back semantics: middle removal keeps the surviving
  slot/worker identity and only stops the popped final slot. Controller hold/toggle decisions use the
  characterized state machine; global Macro Toggle clears active worker state while preserving edge
  latches; each cycle snapshots delay and key-down, then re-reads the output key only after the first
  half-delay for release, matching the frozen live-read point.
- `smu-app` now also owns the live Linux Press a Button (`PressKey`) slice using the shared
  multi-instance container. Each instance keeps its own Z-by-default trigger, D-by-default bound
  output key, foreground gate, 16 ms output hold, and 0 ms bonus delay. Runtime keeps one rising-edge
  latch per instance and resets the entire latch vector on count changes, just like C++. Each rising
  edge snapshots trigger/output/delays before spawning an overlapping one-shot worker. The worker
  preserves the same-key quirk (release the trigger first when trigger and output match), clamps bonus
  delay to nonnegative and hold delay to at least 1 ms through the characterized runtime plan, checks
  the shared running flag only after the optional bonus delay, and otherwise completes output
  hold/release even if shutdown begins during the hold.
- `smu-app` now owns the live Linux Wallhop/Rotation slice and the first shared multi-instance UI
  container. The container preserves the C++ rules used by Wallhop, PressKey, and SpamKey: there is
  always one main instance; new instances append defaults; removing while the main slot is selected
  targets the final extra instance; removing an extra slot targets that slot; and removal requires a
  second click inside the 0.7-second confirmation window. Wallhop sidebar sub-instance selection is
  now live. Runtime state keeps one rising-edge latch per instance and resets the entire latch vector
  whenever the instance count changes, matching `wallhopWasPressed_.assign(size, false)`. Each edge
  snapshots the complete characterized worker plan before spawning an overlapping one-shot worker,
  including instance-local foreground gating and packed bound jump-key injection.
- `smu-app` now owns the live Linux Walless LHJ slice. The F6 controller snapshots the current
  process-target vector at the rising edge, then launches one-shot workers whose internal 645 ms
  sequence is preserved exactly. Side selection is read at worker start; global zoom/shift mode and
  reverse direction are re-read around 595 ms; the release mode and shift key are re-read around
  645 ms. Workers intentionally continue through the sequence if the global macro toggle changes,
  matching C++ generic-worker behavior. The controller also preserves its rising-edge latch while the
  global Macro Toggle is off. The same pause-latch rule was corrected for live Freeze: Freeze resumes
  suspended PIDs when globally disabled but does not reset the toggle edge latch.
- `smu-app` now owns the live Linux Wall-Walk slice. The F1 toggle/hold controller is separated
  from a dedicated motion worker. Each iteration snapshots FPS and the two pre-move side/strength
  reads before the first move, waits one characterized frame delay, performs the second move, then
  re-reads the microsecond tail delay only at the legacy post-move point. The Rust pane also preserves
  C++'s odd sensitivity calibration sign flip between the global-settings pass and the selected
  Wall-Walk section.
- `smu-app` now owns the live Linux Item Clip slice. The F3/slot-7/34-ms defaults feed a 2 ms
  foreground-gated controller and a dedicated worker. Every worker iteration snapshots delay and slot
  before key-down, clamps the half-delay exactly like C++, and always completes down/wait/up/wait before
  checking cancellation again. This is the characterized ItemClip controller/worker migration applied
  to the real native input service.
- `smu-app` now owns the live Linux Speedglitch slice as two long-lived lanes: a 2 ms controller
  retains foreground/hotkey/hold-vs-toggle state while a dedicated motion worker owns the frame waits.
  Each worker cycle snapshots runtime FPS exactly once, reads X immediately before the first move,
  then re-reads Y only after the first wait, preserving the characterized C++ live-read point. The
  Rust global settings state also preserves C++'s unusual startup split: production displays FPS `60`
  while the runtime value starts at `120` until the FPS text buffer is successfully synchronized; the
  Lag Switch parity hook displays `200` without changing that runtime value.
- `smu-app` now also owns the live Linux Item Desync slice. Its Rust pane preserves the F5/slot-5
  defaults and foreground gate, while a dedicated 2 ms controller lane emits the characterized
  down/up/down/up inventory-slot pulse for every active poll. The original C++ controller behavior
  remains frozen separately in the runtime fixture; the migration test proves all four pulse events
  moved to the dedicated lane while the shared controller clock remains unchanged.
- `smu-runtime` includes the characterized Lag Switch controller step as a pure side-effect boundary.
  A generated 140-case C++ oracle corpus freezes section/foreground cleanup, hold/toggle edges,
  backend initialization failure, config-cache publication order, activation start-time resets, and
  float32 auto-unlag thresholds. `smu-app` now owns the first live built-in controller cutover on
  Linux: one long-lived 2 ms controller thread reads the native evdev state service, publishes UI
  config into `smu-network`, and drives the real helper transport. The cutover deliberately preserves
  C++'s separate `bWinDivertEnabled` compatibility flag: hotkey-triggered transport initialization can
  succeed while that UI-enabled flag stays false; manual UI/helper initialization sets it true.
- `smu-network` owns lag-switch config arbitration, the deterministic Linux helper command grammar,
  the deterministic Windows WinDivert filter grammar, and the runtime control capability used by
  scripting. The Windows builder preserves C++ direction/fake-lag capture rules, protocol selection,
  process-UDP-port targeting precedence, Roblox range/dynamic-IP set ordering, and custom IP/port
  clause order without requiring the WinDivert transport to be live. Linux now also has the real root-validated
  `nethelper` Unix-socket transport, applied-command dedup/error semantics, script/base ownership,
  and the one-second dynamic Roblox-target refresh worker; endpoint metadata comes from `smu-roblox`
  instead of duplicating log parsing. `smu-scripting` wires `lagSwitch` / `lagswitch`, config
  get/set/clear, and status through that service with the legacy field/range, owner-token, settings,
  and cleanup semantics. `smu-app` now owns the Linux lagswitch helper-launch boundary: it resolves
  the bundled helper with the same AppImage/current-directory precedence, stages a private 0700 copy,
  binds it to the GUI PID/start-time identity, launches it through graphical `pkexec`, verifies the
  root helper socket, and exposes the default C++ Administrator Required modal. The modal now also
  preserves C++ notification suppression: `suppressed_notifications` keeps arbitrary string IDs,
  legacy `DontShowAdminWarning=true` merges the administrator-warning ID, and saves emit both the
  sorted ID array and compatibility boolean. Changing `Do not show this again` persists immediately;
  future UI/controller helper requests bypass the modal when suppressed, save the active profile
  before elevation, and then reuse the same graphical helper-launch action. Failed helper starts do
  not recursively reopen/relaunch the suppressed prompt.
- `smu-scripting` owns imported-script hotkey/metadata parsing, script-owned hotkey state, persisted
  UI/settings state, and the restricted Lua 5.4.8 VM lifecycle. The VM matches the C++ library
  surface, allocator cap, instruction-hook timeout/cancellation model, protected-call/coroutine stop
  wrappers, and foundational timing/cancellation functions. Service-backed globals now cover direct
  input/text/hotkeys, edge-triggered input callbacks/listeners and managed hotkey mirroring,
  display/pixels/movement, process freeze, lag switch, Roblox log reads/callbacks, `log`,
  `getPlatform`, `getSMUVersion`, and `getSavedValue`. The full legacy `ui` table is also modeled
  without ImGui in the scripting crate: persistent `settings` synchronization, control/ID budgets,
  capture commit/rollback, render-time edits, dynamic text, keybind aliases, and button callbacks
  are exposed through app-facing state/interaction queues. `LuaScriptInstance` now composes those
  adapters into one per-script VM, enforces text-only chunks and the `onExecute` gate, runs initial
  `onSettings`, maps completed/error/timeout/cancelled cleanup reasons, releases managed input and
  callbacks before `onCleanup`, clears lag-switch ownership afterward, and intentionally leaves a
  script freeze in place until instance shutdown just like C++. `ScriptManager` now owns imported
  record/path normalization, import-from-save quirks, reload/reset/remove, persistence, live hotkey
  and process-target handles, and named per-record execution workers. Different scripts can execute
  concurrently; the same record is protected by an internal active reservation, while public
  `running` is published only after the Lua budget/cancellation state is armed so Force Stop cannot
  be lost during worker startup. Render-mode `onSettings` uses the C++ try-lock/skip behavior. Saved
  app-setting scalars are exposed by `smu-profile`; transient active-monitor values stay in
  `smu-display`. Settings render/callback mode is treated as one restricted scripting phase,
  matching the C++ `IsSettingsCallbackActive` helper.

## Architecture rules

- UI code observes application state and sends commands; it does not call OS APIs directly.
- Platform implementations do not know about Dear ImGui or built-in macros.
- `Input` is not a synonym for display/capture. Monitor geometry and pixel capture are a separate
  capability.
- Platform selection and dependency wiring happen at the application composition root.
- No generic "run arbitrary worker" API is exposed to macro code. Threads have named ownership and
  cleanup semantics.
- Persist the existing settings format during the compatibility phase. A new schema is a later,
  separately tested change.

## Timing contract

SMU contains timing-sensitive behavior down to microseconds. Porting `sleep_for` calls mechanically
is not sufficient, but neither is replacing them with a permanent busy loop.

Timing work follows three layers:

1. **Logical trace tests** use a fake clock and assert exact requested delays/action ordering.
2. **Timing primitive benchmarks** measure real wake-up error on each supported OS.
3. **End-to-end macro tests** allow explicit platform-specific timing tolerances while still
   requiring exact action order and cleanup.

A high-precision wait may combine an efficient blocking sleep with a short final yield/spin phase.
Any spin threshold must be measured, bounded, cancellation-aware, and documented. The runtime
architecture remains open to either one deterministic scheduler or a small number of deliberate
workers until measurements prove which preserves current behavior best. After a controller stall has
been differentially characterized, it may be intentionally migrated to a named worker when doing so
removes unrelated macro interference; preserve its internal action/wait order and add a lane-topology
regression test. HHJ motion, Speedglitch, ItemClip, WallWalk, Windows Anti-AFK, Linux Freeze, and
Linux Item Desync are approved migrations of this kind. The live Linux Speedglitch cutover now uses
that approved controller/worker split directly. Freeze's 50 ms auto-unfreeze override is modeled on a
separate dedicated lane: its worker clock advances exactly 50 ms while the controller clock remains
at zero, preserving the internal unfreeze/wait/resuspend order without stalling Lag Switch.

## UI parity contract

The C++ UI currently includes hand-drawn `ImDrawList` geometry, including the footer/content border
extension in `app/app_ui.cpp`. Those pixels are part of the current visual contract even if a later
implementation can produce the same appearance more cleanly.

Parity tests compare fixed-state C++ and Rust captures at a matrix of window sizes and display
scales. Live process discovery is disabled during parity capture, so running Sober/Roblox instances
cannot change the status indicator. Ordinary fixtures keep the C++ oracle's `processFound = false`;
a paired deterministic process-status fixture forces found/not-found on both sides without touching
live discovery and differs only in the 10x10 header status dot. Linux now has automated minimum-size
captures for the Lag Switch pane, an open Settings window, the Theme
Editor, and the profile drop-up menu. The Settings case keeps updater work suppressed while preserving
C++'s enabled/not-yet-checked visual state, and it caught the missing `Select a section to see its
settings.` empty-pane message plus the legacy 4 px Old Unicode Chat-Typing label offset. The Theme
Editor case caught a full-line `NewLine`/`Spacing` drift and Dear ImGui 1.92.9's new per-channel
`ColorEdit4` markers; Rust uses `Spacing()` and disables only those newer markers to preserve the C++
1.92.5 oracle. The profile-menu case uses an in-memory `(default)` catalog on both sides and caught
Rust's phantom minimum-three-row sizing, missing 5 px popup padding/`SeparatorTextAlign.y` height
term, and disabled Delete/Duplicate buttons for `(default)`; the live default-duplicate path was
already present, while deletion remains protected by persistence. A separate 1280x1200 short-
description capture now gates the sidebar's real no-scroll branch: it caught Rust treating
`CalcTextSize("")` as zero height instead of Dear ImGui's `(0, font_size)`, an 8px imported-script
width error from missing frame-padding subtraction, and a one-pixel built-in/imported separator
offset. A Freeze-selected built-in-footer capture then exposed Rust treating every X11 session as
foreground-capable; the shared process/UI capability now matches C++'s EWMH probe and forces the
foreground-dependent checkbox into the disabled/unchecked fallback when those properties are absent.
An in-memory imported-script footer capture then caught Rust using the Settings-menu keybind geometry
instead of C++'s default 170/130 unwrapped layout, formatting the script-unbound sentinel as an
ordinary zero key instead of `UNBOUND`, and collapsing the distinction between no `onSettings()`
callback and a callback that returns no controls; it also removed the stale Rust-only profile-
persistence warning from a path whose persistence is already attached. The import-trust preview is
now gated with an in-memory Lua source fixture as well: Rust ports C++'s centered 760x560 constrained
modal, exact disclaimer/section heading, bordered horizontal-scroll syntax preview, visible-line
clipping, metadata/comment/string/keyword/function/risky-function coloring, and the matching
error-colored embedded-NUL rejection branch. Parity capture `DeltaTime` is stabilized at 1/60s so
Dear ImGui modal dimming reaches the same third-frame state as
the C++ 60 FPS oracle. A warning-toast capture now gates a real Rust warning FIFO and the C++-matching
top-right selectable/Dismiss surface; Linux startup feeds the existing foreground-detection fallback
warning through that queue. The deterministic toast fixture stays single-line because Dear ImGui
rewrote punctuation-aware word wrapping between 1.92.5 and 1.92.9, and the oracle should gate the
application surface rather than an upstream line-break implementation. Tutorial media now uses the
same four JPEG assets as C++ through managed ImGui textures, with packaged/AppDir/source-tree lookup,
centered aspect-preserving sizing, and warning-FIFO fallback when an asset cannot be loaded. The
Laugh Clip tutorial-media oracle swaps only the JPEG for an identical 248x140 placeholder on both
sides, so image-slot/layout parity is deterministic without comparing decoder output. Companion
Wallhop, Item Clip, Wall Walk, and Spam Key captures reuse that placeholder to gate each tutorial
module body. Wallhop parity exposed three Rust production drifts: the missing IMPORTANT sensitivity
note, an auto-sized Jump Key reset button instead of C++'s fixed 25px control, and a first-frame
sensitivity sync that converted the canonical default 300px flick to 600px from stale 150-degree
text. Rust now treats startup/profile sensitivity as already applied and only recalibrates Wallhop
pixels after an actual sensitivity edit, matching C++. The remaining nine built-in module bodies are
also gated directly. Those captures exposed HHJ and Floor Bounce advanced controls that Rust had
rendered unconditionally instead of using C++'s closed-by-default collapsing headers, the missing HHJ
`Reference video coming soon!` tail line, and the hard-coded Bunnyhop title instead of the catalog's
`Smart Bunnyhop`. Speedglitch differs at one isolated 1x1 text anti-aliasing pixel; all other default
module-body gates need no masks beyond the shared footer seam. The 1280x800 theme-shell matrix now
gates Cyberpunk idle/hover, Default Dark, and Sunset Orange; the hover fixture injects the same mouse
position before `NewFrame` on both apps and exercises the real sidebar hover path. Default Dark needs
only three measured zero-rounding edge masks, while Sunset Orange needs four tiny rounded footer
corner masks. Linux permission/policy modals are also gated directly: Administrator Required,
Additional Freeze Permission Required, Wayland Screen Capture, Wayland Absolute Mouse Control, and
Linux Input Setup Required. Rust now mirrors the C++ modal center/size constraints,
`AlwaysAutoResize`, close-button semantics, spacing, default focus, and the input-setup modal's 580px
fixed-wrap unformatted details/action regions. The input-setup fixture uses deterministic in-memory
permission summary/details text and bypasses only the live permission snapshot during parity capture;
only measured rounded corners plus one stable 1x2 dimmed-text threshold fragment on Administrator
Required are masked. The Update Available modal is now gated in both branches: deterministic
`3.4.0 -> 9.9.9` fixtures cover auto-installable `Yes`/`No`/`Cancel` and manual-only `Open latest
release`/`Not now`, without invoking updater transport, downloads, or browser launches. Rust now
matches C++'s centered 520px `AlwaysAutoResize` modal and default-focus placement; only the measured
rounded modal corners are masked. Critical Platform Error is also gated through the production FIFO
modal/support text. Rust now has a real critical-notification queue, and profile-save failures enqueue
it instead of being silently dropped; the parity fixture uses one deterministic critical message and
only the four measured rounded modal corners are masked. Fractional-scale manifest rows remain manual OS/DPI
references because Xvfb plus SDL's X11 scaling override produced pixel-identical 1x/1.25x captures
instead of exercising a real content-scale change. Exact RGBA equality is the default. Masks are
permitted only for explicitly documented regions where the oracle itself is intentionally excluded;
a mask must be narrow and reviewed.

Named layout constants are preferred in Rust even when their numeric values must match existing
magic constants. Pixel parity is the requirement, not preservation of the old implementation style.

## Branch/cutover policy

- `main` remains the production C++ line during the rewrite.
- Important C++ fixes can be cherry-picked into `rewrite/rust` while the oracle still exists.
- Do not delete the C++ tree early; same-checkout differential testing is intentional.
- The final cutover is a normal reviewed merge/replacement after tests prove parity.
- Local commits are expected on `rewrite/rust`; do not push without explicit approval.
