# Screenshot provenance

- `lag-switch-user-reference-872x580.png`
  - Image size: 872x580.
  - Source: user-provided screenshot/crop from the current C++ SMU UI.
  - Do not infer the application client-window size from this image; current SMU enforces a
    1180x700 minimum on Windows/Linux and 1220x700 on macOS.
  - Relevant detail: the right-pane/footer boundary is manually reconstructed with `ImDrawList`
    geometry in `app/app_ui.cpp`. Rust may implement the appearance more cleanly, while parity tests
    keep its geometry strict and permit only a narrow one-pixel raster exception around that seam.
