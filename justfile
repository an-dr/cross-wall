# Shortcuts for the cargo commands in the README and CI. Run `just` to list them.

set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# List the recipes.
default:
    @just --list

# Fetch the vendored bones and its own submodules.
submodules:
    git submodule update --init --recursive

# Build and start the app.
run:
    cargo run -p cross-wall

# Build every crate.
build:
    cargo build --workspace

# Run every test.
test:
    cargo test --workspace

# Format the code.
fmt:
    cargo fmt

# Check formatting and lints as CI does.
lint:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings

# Run the checks CI runs before packaging.
check: lint test

# This machine's release platform, the default for `package` and `dist`.
host_platform := os() + "-" + if arch() == "aarch64" { "arm64" } else { "x64" }

# Build a release zip and its update manifest into release/; platform: windows-x64, windows-arm64, or linux-x64.
package platform=host_platform:
    cargo xtask package --platform {{platform}}

# Unpack this machine's release into a fresh dist/, ready to run.
[unix]
dist:
    zip="$(cargo xtask package --platform {{host_platform}} | tail -n 1)" && rm -rf dist && mkdir dist && unzip -q "$zip" -d dist && echo "Run dist/cross-wall-app"

# Unpack this machine's release into a fresh dist/, ready to run.
[windows]
dist:
    $zip = cargo xtask package --platform {{host_platform}} | Select-Object -Last 1; if (Test-Path dist) { Remove-Item -Recurse -Force dist }; Expand-Archive $zip dist; "Run dist\cross-wall-app.exe"

# List the Linux packages bones needs to build SDL3.
[linux]
linux-packages:
    cd vendor/bones && cargo xtask linux-packages
