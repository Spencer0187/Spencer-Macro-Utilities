# macOS Unsigned Release Notes

Use this text in GitHub release notes for macOS artifacts:

SMU for macOS is not Apple Developer ID signed and is not notarized. macOS will warn on first launch. Drag `suspend.app` to Applications, try to open it, then approve it in System Settings > Privacy & Security > Open Anyway.

After launch, grant Accessibility and Screen Recording in the in-app setup screen. If permissions look enabled in System Settings but SMU still reports them missing after an update, use Reset macOS Permission Entries in SMU, approve the app again, and restart.

Do not disable Gatekeeper globally. If quarantine gets stuck after using Open Anyway, run:

```bash
xattr -dr com.apple.quarantine /Applications/suspend.app
```
