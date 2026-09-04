# Input compatibility fixtures

`hotkey_cases.json` freezes the C++ hotkey predicate behavior used by `smu-input`.

`linux_keymap.json` is generated from the production C++ `core/keymapping.h` tables plus the Linux backend's explicit left/right modifier handling. It covers every historical SMU virtual-key value from 0 through the two wheel sentinels (257), and every Linux evdev key code below `KEY_MAX` (`0x2ff`). Regenerate it with:

```sh
cmake --build build --target smu_compat_oracle -j2
python3 tools/update_compat_fixtures.py linux-keymap
```

The Rust Linux input backend consumes this fixture in tests so mapping drift is caught without requiring the C++ oracle during `cargo test`.
