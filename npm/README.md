# stacksdapp npm wrapper

This folder is an npm “shim” for the Rust CLI binary (`stacksdapp`).

The npm package does not bundle Rust. Instead, on first run it downloads the matching prebuilt binary from GitHub Releases and caches it under `~/.cache/stacksdapp/`.

## Publish

From this folder:

```bash
cd npm
npm publish
```

## Expected release assets

Assets must exist on GitHub Releases for tags like `v0.1.3` (same as the wrapper version).

File names expected by the launcher:

- `stacksdapp-x86_64-apple-darwin`
- `stacksdapp-aarch64-apple-darwin`
- `stacksdapp-x86_64-unknown-linux-gnu`
- `stacksdapp-x86_64-pc-windows-msvc.exe`


