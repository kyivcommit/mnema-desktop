#!/usr/bin/env bash
# Uninstall every rustup toolchain except the one rust-toolchain.toml names,
# and make that one the default. For CI, before `Swatinem/rust-cache`.
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
# changed, and `bundle` rebuilt `tauri-cli` from source for 6–8 minutes.
# With the image's toolchain gone, the key hashes the pinned version alone.
#
# The default is set explicitly because uninstalling the image's `stable`
# removes rustup's default with it, and a cargo invoked outside a directory
# carrying rust-toolchain.toml would then refuse to run at all.
set -euo pipefail

active=$(rustup show active-toolchain | cut -d' ' -f1)
[ -n "$active" ] || { echo "prune-toolchains: no active toolchain" >&2; exit 1; }

rustup toolchain list | cut -d' ' -f1 | while read -r toolchain; do
  [ "$toolchain" = "$active" ] && continue
  rustup toolchain uninstall "$toolchain"
done
rustup default "$active"
rustup toolchain list
