# Viewer third-party notices

The ARMHF package includes these notices under `viewer/licenses/`.

- [Linked libraries and runtime](THIRD-PARTY.txt) covers the native dependencies resolved by `Cargo.lock`, the
  shared PrimitiveOS Tools code, the Rust runtime, and musl.
- [PDF renderer and bundled resources](pdf/README.md) covers Hayro’s standard fonts and character maps.

Keep the notices in sync with the release’s ARMHF dependency graph. The graph can be inspected with
`cargo metadata --locked --format-version 1 --filter-platform armv7-unknown-linux-musleabihf`.
