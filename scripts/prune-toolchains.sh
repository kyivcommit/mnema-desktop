#!/usr/bin/env bash
# Uninstall every rustup toolchain except the one rust-toolchain.toml names,
# and make that one the default. CI only, before `Swatinem/rust-cache`; it
# refuses to run without `CI` in the environment, because it is destructive
# and lives in the directory docs/BUILD.md sends developers to.
#
# Why: the action's cache key hashes `rustc -vV` of EVERY toolchain in
# `rustup toolchain list` (rust-cache v2.9.1, src/config.ts, getRustVersions),
# not only the active one — and a hosted runner image ships a `stable` of its
# own that moves with the image. Read from two runs' own `Cache Configuration`
# groups on 2026-09-02: the same commit hashed `1.96.0 … ac68faa` on
# `macos-14` image 20260629.0180.1 and `1.98.0 … 88d9e12` on image
# 20260831.0302.1, beside the pinned 1.97.1 on both — so `check (macos-14)`
# and `bundle` got different keys (`…-5e642cdd-…` against `…-c3c8feb3-…`)
# for an environment in which nothing this repository builds with had
# changed. What that cost at the time was a `cargo install tauri-cli` from
# source, 6–8 minutes; the same change that added this script replaced that
# install with a pinned download, so what a moving key costs now is a cold
# `target/` — unmeasured for `check (macos-14)` and `bundle`, 3:02–3:23 per
# leg where `mutations` measured it. With the image's toolchain gone, the
# key hashes the pinned version alone.
#
# Two consequences to know before reading a run. Every job's key changes
# ONCE, on the first run after this lands — a full miss on `check`,
# `mutations` and `bundle` alike, and open pull requests restore nothing
# from `main` until that run has saved. And the survivor is checked against
# rust-toolchain.toml rather than trusted: `rustup show active-toolchain`
# answers for the working directory, and run from anywhere without that
# file it would name the image's `stable` — and this script would then
# uninstall the pinned toolchain and keep the moving one, green.
#
# The default is set explicitly because uninstalling the image's `stable`
# removes rustup's default with it, and a cargo invoked outside a directory
# carrying rust-toolchain.toml would then refuse to run at all.
set -euo pipefail

[ -n "${CI:-}" ] || {
  echo "prune-toolchains: refusing outside CI — this uninstalls every rustup toolchain but one" >&2
  exit 1
}

repo=$(git rev-parse --show-toplevel)
channel=$(sed -nE 's/^channel *= *"([^"]+)"/\1/p' "$repo/rust-toolchain.toml")
active=$(rustup show active-toolchain | cut -d' ' -f1)
case "$active" in
  "$channel"-*) ;;
  *)
    echo "prune-toolchains: the active toolchain is '$active', but rust-toolchain.toml names '$channel';" >&2
    echo "refusing to uninstall around the wrong survivor (run from the checkout, after 'rustup toolchain install')" >&2
    exit 1
    ;;
esac

rustup toolchain list | cut -d' ' -f1 | while read -r toolchain; do
  [ "$toolchain" = "$active" ] && continue
  rustup toolchain uninstall "$toolchain"
done
rustup default "$active"
rustup toolchain list
