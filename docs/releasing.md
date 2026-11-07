# Building and releasing

`cargo xtask package --platform <windows-x64|windows-arm64|linux-x64>` builds a release and writes the zip and its update manifest to `release/`. It builds for the machine it runs on unless given `--target <triple>`.

To publish, set the version in `[workspace.package]` of the root `Cargo.toml` and `UPGRADER_LAUNCHER_VERSION` in `.cargo/config.toml` to the same value, get that commit onto `main`, and push a matching tag. `cargo test` and `cargo xtask package` both fail when the two versions differ.

Then tag the commit and push it:

```sh
git tag v0.2.0
git push origin main v0.2.0
```

The [build workflow](../.github/workflows/build.yml) tests and packages on all three platforms, then publishes a GitHub release. A tag that does not match the version fails the build. Installed apps look for updates in the latest release of `an-dr/cross-wall`, so the repository must be public for them to work.
