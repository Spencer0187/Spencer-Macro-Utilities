# Hotkey compatibility fixtures

`cases.json` is generated from the production C++ parser through `smu_compat_oracle`, not by the
Rust implementation. It freezes the imported-script `@keybind` grammar and packed integer result.

Regenerate after an intentional C++ compatibility change with:

```bash
cmake -S . -B build
cmake --build build --target smu_compat_oracle
python3 tools/update_compat_fixtures.py hotkeys
```
