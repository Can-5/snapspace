# SnapSpace

**Workspace profile capture and validation for Orbit — honest restore planning.**

> Orbit companion module — phase 2/4 of the [Orbit ecosystem](https://github.com/Can-5/orbit).
> Standalone crate: usable without Orbit, without other modules, and without network.

## Implemented now

- WorkspaceProfile: window positions per display, with role annotations for apps/links
- validate_profile(): bounds check against displays + overlap detection with fixes suggested
- plan_restore(): per-window outcome — Ready / MissingApp / OutOfBounds / Refused — never silent failure
- Full multi-monitor geometry math (left/top/right/bottom, intersections)

## Not implemented yet (explicitly out of scope for this crate)

- No window manager hooks — KWin/X11/Wayland scripting lives in the Orbit host (WindowManagement permission)
- No auto-apply; restore is always user-confirmed

## Build & test

```sh
cargo test  # 6 tests
```

Dependencies: serde, serde_json, thiserror. No cross-repo path dependencies, no Git dependencies, no downloads at build time beyond crates.io.

## Privacy

Profile data never leaves the device. There is no telemetry and no network code.

## License

MIT © The Orbit contributors. See [LICENSE](./LICENSE).
