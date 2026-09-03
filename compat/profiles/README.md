# Profile/config compatibility corpus

`cases.json` is generated from the production C++ profile implementation by
`tools/update_compat_fixtures.py profiles` and `smu_profile_compat_oracle`.
The Rust `smu-profile` crate consumes it as a differential compatibility suite.

The corpus currently freezes:

- exact current-container and legacy-flat format detection, including the C++-generated 114-key legacy recognizer set;
- startup profile-selection precedence, including `(default)` handling and lexicographic fallback semantics;
- profile-name enumeration and low-level duplicate/rename/delete quirks;
- one-generation `.bak` rollback/recovery behavior;
- settings discovery precedence across canonical, executable, executable-parent, and current directories;
- global `SMCSettings.json`-before-`RMCSettings.json` precedence;
- primary-vs-backup precedence, backup self-healing, corrupt-primary preservation, and schema-blind acceptance of any parseable JSON during discovery.

All generated filesystem cases run in temporary directories. They must never use
the real user settings path.

Regenerate after an intentional C++ compatibility change:

```sh
cmake --build build --target smu_profile_compat_oracle
python3 tools/update_compat_fixtures.py profiles
cargo test --manifest-path rust/Cargo.toml -p smu-profile
```

`rust/crates/smu-profile/src/generated_legacy_keys.rs` is generated from the same
C++ oracle so production Rust code cannot silently broaden or narrow the legacy
flat-file detector.
