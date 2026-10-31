import importlib.util
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "stage_rust_macos_app.py"
SPEC = importlib.util.spec_from_file_location("stage_rust_macos_app", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RustMacosStagingTest(unittest.TestCase):
    def test_product_identity_and_universal_targets_match_release_contract(self) -> None:
        self.assertEqual(MODULE.APP_BUNDLE_NAME, "Spencer Macro Utilities.app")
        self.assertEqual(MODULE.APP_EXECUTABLE_NAME, "Spencer-Macro-Utilities")
        self.assertEqual(MODULE.APP_CARGO_BINARY_NAME, "smu-app")
        self.assertEqual(MODULE.HELPER_EXECUTABLE_NAME, "SMU-Update-Helper")
        self.assertEqual(MODULE.HELPER_CARGO_BINARY_NAME, "smu-macos-updater-helper")
        self.assertEqual(MODULE.BUNDLE_IDENTIFIER, "com.spencer0187.smu")
        self.assertEqual(
            MODULE.TARGET_ARCHITECTURES,
            {
                "aarch64-apple-darwin": "arm64",
                "x86_64-apple-darwin": "x86_64",
            },
        )
        self.assertEqual(MODULE.FRAMEWORK_RPATH, "@executable_path/../Frameworks")

    def test_profile_args_are_explicit(self) -> None:
        self.assertEqual(MODULE.profile_args("debug"), [])
        self.assertEqual(MODULE.profile_args("release"), ["--release"])
        with self.assertRaises(ValueError):
            MODULE.profile_args("relwithdebinfo")

    def test_resolve_sdl_dylib_prefers_versioned_profile_root_copy(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            profile = target / "release"
            profile.mkdir()
            versioned = profile / "libSDL3.0.dylib"
            versioned.write_bytes(b"versioned")
            plain = profile / "libSDL3.dylib"
            plain.symlink_to(versioned.name)
            fallback = target / "release/build/sdl3-sys-abc/out/lib/libSDL3.0.dylib"
            fallback.parent.mkdir(parents=True)
            fallback.write_bytes(b"fallback")

            self.assertEqual(MODULE.resolve_sdl_dylib(target, "release"), versioned.resolve())

    def test_resolve_sdl_dylib_accepts_one_build_output_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            fallback = target / "debug/build/sdl3-sys-abc/out/lib/libSDL3.0.dylib"
            fallback.parent.mkdir(parents=True)
            fallback.write_bytes(b"fallback")
            self.assertEqual(MODULE.resolve_sdl_dylib(target, "debug"), fallback.resolve())

    def test_resolve_sdl_dylib_rejects_ambiguous_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            profile = target / "release"
            profile.mkdir()
            (profile / "libSDL3.0.dylib").write_bytes(b"one")
            (profile / "libSDL3.1.dylib").write_bytes(b"two")
            with self.assertRaises(RuntimeError):
                MODULE.resolve_sdl_dylib(target, "release")

    def test_linked_libraries_deduplicates_universal_otool_sections(self) -> None:
        universal_otool = """/tmp/app (architecture x86_64):
\t@rpath/libSDL3.0.dylib (compatibility version 0.0.0, current version 0.0.0)
\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)
/tmp/app (architecture arm64):
\t@rpath/libSDL3.0.dylib (compatibility version 0.0.0, current version 0.0.0)
\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)
"""
        completed = mock.Mock(stdout=universal_otool)
        with mock.patch.object(MODULE, "run", return_value=completed):
            self.assertEqual(
                MODULE.linked_libraries(Path("/tmp/app")),
                ["@rpath/libSDL3.0.dylib", "/usr/lib/libSystem.B.dylib"],
            )

    def test_info_plist_preserves_cpp_identity_and_deployment_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            bundle = Path(directory) / MODULE.APP_BUNDLE_NAME
            (bundle / "Contents").mkdir(parents=True)
            info = MODULE.write_info_plist(bundle, "3.4.0")
            with info.open("rb") as stream:
                plist = plistlib.load(stream)

            self.assertEqual(plist["CFBundleDisplayName"], "Spencer Macro Utilities")
            self.assertEqual(plist["CFBundleExecutable"], MODULE.APP_EXECUTABLE_NAME)
            self.assertEqual(plist["CFBundleIdentifier"], "com.spencer0187.smu")
            self.assertEqual(plist["CFBundleIconFile"], "smu_icon.icns")
            self.assertEqual(plist["CFBundleShortVersionString"], "3.4.0")
            self.assertEqual(plist["CFBundleVersion"], "3.4.0")
            self.assertEqual(plist["LSMinimumSystemVersion"], "10.15")

    def test_stage_bundle_creates_rust_runtime_layout_and_notices(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            apps = []
            helpers = []
            sdls = []
            for index in range(2):
                app = temp / f"app-{index}"
                helper = temp / f"helper-{index}"
                sdl = temp / f"arch-{index}" / "libSDL3.0.dylib"
                sdl.parent.mkdir()
                app.write_bytes(f"app-{index}".encode())
                helper.write_bytes(f"helper-{index}".encode())
                sdl.write_bytes(f"sdl-{index}".encode())
                apps.append(app)
                helpers.append(helper)
                sdls.append(sdl)

            bundle = temp / MODULE.APP_BUNDLE_NAME

            def fake_lipo(inputs: list[Path], output: Path) -> None:
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_bytes(b"\n".join(path.read_bytes() for path in inputs))

            with (
                mock.patch.object(MODULE, "create_universal_binary", side_effect=fake_lipo),
                mock.patch.object(MODULE, "rewrite_sdl_dependency"),
                mock.patch.object(MODULE, "require_universal_binary"),
            ):
                staged_app, staged_helper, staged_sdl = MODULE.stage_bundle(
                    ROOT,
                    apps,
                    helpers,
                    sdls,
                    bundle,
                    "3.4.0",
                )

            self.assertEqual(staged_app, bundle / "Contents/MacOS/Spencer-Macro-Utilities")
            self.assertEqual(staged_helper, bundle / "Contents/MacOS/SMU-Update-Helper")
            self.assertEqual(staged_sdl, bundle / "Contents/Frameworks/libSDL3.0.dylib")
            self.assertTrue((bundle / "Contents/MacOS/assets").is_symlink())
            self.assertEqual((bundle / "Contents/MacOS/assets").readlink().as_posix(), "../Resources/assets")
            self.assertTrue((bundle / "Contents/MacOS/assets/LSANS.TTF").is_file())
            self.assertTrue((bundle / "Contents/MacOS/assets/macro_tutorials/wallhop.jpg").is_file())
            self.assertTrue((bundle / "Contents/Resources/assets/LSANS.TTF").is_file())
            self.assertTrue((bundle / "Contents/Resources/smu_icon.icns").is_file())
            for _, relative in MODULE.required_notice_files(ROOT):
                self.assertTrue((bundle / "Contents/Resources" / relative).is_file(), relative)

    def test_validate_bundle_requires_universal_app_helper_sdl_and_sdl_free_helper(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            bundle = Path(directory) / MODULE.APP_BUNDLE_NAME
            macos = bundle / "Contents/MacOS"
            frameworks = bundle / "Contents/Frameworks"
            resources = bundle / "Contents/Resources"
            (resources / "assets/macro_tutorials").mkdir(parents=True)
            macos.mkdir(parents=True)
            (macos / "assets").symlink_to("../Resources/assets", target_is_directory=True)
            frameworks.mkdir(parents=True)
            app = macos / MODULE.APP_EXECUTABLE_NAME
            helper = macos / MODULE.HELPER_EXECUTABLE_NAME
            sdl = frameworks / "libSDL3.0.dylib"
            for path in (app, helper, sdl, macos / "assets/LSANS.TTF"):
                path.write_bytes(b"x")
            for path in (
                resources / "smu_icon.icns",
                resources / "LICENSE",
                resources / "PRIVACY.md",
                resources / "THIRD_PARTY_NOTICES.md",
            ):
                path.write_bytes(b"x")
            MODULE.write_info_plist(bundle, "3.4.0")

            def linked(path: Path) -> list[str]:
                if path == app:
                    return ["@rpath/libSDL3.0.dylib", "/usr/lib/libSystem.B.dylib"]
                if path == helper:
                    return ["/usr/lib/libSystem.B.dylib"]
                return []

            with (
                mock.patch.object(MODULE, "linked_libraries", side_effect=linked),
                mock.patch.object(MODULE, "rpaths", return_value=[MODULE.FRAMEWORK_RPATH]),
                mock.patch.object(MODULE, "require_universal_binary"),
            ):
                MODULE.validate_bundle(bundle, "3.4.0")

            with (
                mock.patch.object(
                    MODULE,
                    "linked_libraries",
                    side_effect=lambda path: ["@rpath/libSDL3.0.dylib"] if path in (app, helper) else [],
                ),
                mock.patch.object(MODULE, "rpaths", return_value=[MODULE.FRAMEWORK_RPATH]),
                mock.patch.object(MODULE, "require_universal_binary"),
            ):
                with self.assertRaisesRegex(RuntimeError, "helper must remain SDL-free"):
                    MODULE.validate_bundle(bundle, "3.4.0")

    def test_ad_hoc_signing_stages_all_code_objects(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            bundle = Path(directory) / MODULE.APP_BUNDLE_NAME
            frameworks = bundle / "Contents/Frameworks"
            macos = bundle / "Contents/MacOS"
            frameworks.mkdir(parents=True)
            macos.mkdir(parents=True)
            (frameworks / "libSDL3.0.dylib").write_bytes(b"sdl")
            (macos / MODULE.APP_EXECUTABLE_NAME).write_bytes(b"app")
            (macos / MODULE.HELPER_EXECUTABLE_NAME).write_bytes(b"helper")
            with self.assertRaises(ValueError):
                MODULE.sign_bundle(bundle, "")
            with mock.patch.object(MODULE, "run") as command:
                MODULE.sign_bundle(bundle, "-")
            self.assertEqual(command.call_count, 5)
            for call in command.call_args_list[:4]:
                self.assertIn("--sign", call.args[0])
                self.assertIn("-", call.args[0])
            self.assertEqual(command.call_args_list[-1].args[0][1:4],
                             ["--verify", "--deep", "--strict"])

    def test_rewrite_ci_builds_signs_validates_and_packages_universal_bundle(self) -> None:
        workflow = (ROOT / ".github/workflows/rust-rewrite.yml").read_text(encoding="utf-8")
        for expected in (
            '"assets/**"',
            '"version"',
            '"third_party/**"',
            '"scripts/stage_rust_macos_app.py"',
            '"tests/test_rust_macos_staging.py"',
            'rustup target add --toolchain 1.98.0 aarch64-apple-darwin x86_64-apple-darwin',
            'python3 -m unittest tests.test_rust_macos_staging tests.test_rust_macos_updater',
            'scripts/stage_rust_macos_app.py',
            '--bundle "build/rust-macos-app/Spencer Macro Utilities.app"',
            '--sign-identity -',
            'lipo -archs "$code"',
            'grep -qw arm64',
            'grep -qw x86_64',
            'codesign --verify --deep --strict --verbose=2 "$app"',
            "official_update_assets(sys.argv[1])[\"macos-universal\"]",
            'ditto -c -k --sequesterRsrc --keepParent',
            '--only-platform macos-universal',
            'unzip -t "$release_dir/$asset"',
            "Spencer Macro Utilities.app/Contents/Info.plist",
            'name: rust-macos-universal-test-adhoc',
        ):
            self.assertIn(expected, workflow)
        self.assertNotIn(
            'UpdateChecker::new(CurlUpdateTransport::macos(), current_platform(), true)',
            workflow,
        )

    def test_macos_cutover_switches_remain_explicit_until_production_promotion(self) -> None:
        production = (ROOT / ".github/workflows/build.yml").read_text(encoding="utf-8")
        draft_release = (ROOT / ".github/workflows/draft-release.yml").read_text(encoding="utf-8")
        rewrite = (ROOT / ".github/workflows/rust-rewrite.yml").read_text(encoding="utf-8")
        updater = (
            ROOT / "rust" / "crates" / "smu-app" / "src" / "updater_service.rs"
        ).read_text(encoding="utf-8")

        self.assertIn("run: bash scripts/package_rust_macos_release.sh", production)
        self.assertIn("SMU_MACOS_SIGN_IDENTITY: '-'", production)
        self.assertIn("scripts/stage_rust_macos_app.py", rewrite)
        self.assertIn(
            "UpdateChecker::new(CurlUpdateTransport::macos(), current_platform(), false)",
            updater,
        )
        self.assertNotIn(
            "UpdateChecker::new(CurlUpdateTransport::macos(), current_platform(), true)",
            updater,
        )
        self.assertIn("launch_macos_update_helper(&target_version)", updater)
        self.assertIn("if !state.status.auto_apply_supported", updater)

        for expected in (
            "Apply V3.4.0 legacy ZIP casing",
            'Spencer-Macro-Utilities-V${version}-macOS-universal.zip',
            'Spencer-Macro-Utilities-V${version}-macOS-universal.ZIP',
            "python3 scripts/generate_update_manifest.py",
            "Verify V3.2 legacy updater asset visibility",
        ):
            self.assertIn(expected, draft_release)

    def test_source_contract_builds_and_lipos_both_architectures(self) -> None:
        source = SCRIPT.read_text(encoding="utf-8")
        for expected in (
            '"aarch64-apple-darwin": "arm64"',
            '"x86_64-apple-darwin": "x86_64"',
            'env.setdefault("MACOSX_DEPLOYMENT_TARGET", "10.15")',
            '"--locked"',
            '"-p",\n        "smu-app"',
            '"-p",\n        "smu-macos-updater-helper"',
            '"--target",\n        target',
            'run(["/usr/bin/lipo", "-create"',
            'run(["/usr/bin/lipo", "-archs"',
            '"@executable_path/../Frameworks"',
            '"/usr/bin/install_name_tool"',
            '"/usr/bin/otool"',
            '"/usr/bin/codesign"',
            '"--deep", "--strict"',
            'macos / "assets"',
            'macos / HELPER_EXECUTABLE_NAME',
            'frameworks / next(iter(sdl_names))',
            "helper must remain SDL-free",
        ):
            self.assertIn(expected, source)


if __name__ == "__main__":
    unittest.main()
