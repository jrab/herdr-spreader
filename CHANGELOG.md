# Changelog

## 0.3.4

- Make the Herdr `Apply layout` action reuse the invoking workspace, tab, and
  pane instead of creating a duplicate workspace.

## 0.3.3

- Use Herdr's current `pane wait-output` command when synchronizing pane
  startup commands.

## 0.3.2

- Allow `apply-existing` callers to pass an authoritative absolute root so
  shell startup hooks cannot redirect newly split panes into a transient cwd.

## 0.3.1

- Add `apply-existing` for applying one configured layout to a supplied Herdr
  workspace, first tab, and root pane without creating a duplicate workspace.

## 0.3.0

- Add recursive `layout` trees for arbitrary nested pane arrangements.
- Add true balanced 2×2 grid support and a complete grid example.
- Preserve the existing chained `panes` configuration format.
- Validate binary split arity and reject tabs that mix `panes` and `layout`.

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1](https://github.com/yuk1ty/herdr-spreader/compare/v0.2.0...v0.2.1) - 2026-08-16

### Fixed

- call `herdr pane wait-output` instead of `herdr wait output`

## [0.2.0](https://github.com/yuk1ty/herdr-spreader/compare/v0.1.1...v0.2.0) - 2026-07-16

### Added

- *(cli)* add --dry-run flag to print plan without executing
- *(engine)* introduce BackendOp/PaneHandle Data types and plan_workspace Calculation
- add validation checking phase and remove validate plugin

### Other

- Document --dry-run flag in README
- Add functional programming guidelines with Actions/Calculations/Data principle
- *(ARCHITECTURE)* sync to refactored Actions/Calculations/Data architecture
- *(integration)* add plan-pinning tests for end-to-end plan verification
- *(cli)* thread HERDR_SOCKET_PATH into CliBackend; extract choose_focus_strategy
- *(config)* make path resolution immutable

## [0.1.1](https://github.com/yuk1ty/herdr-spreader/compare/v0.1.0...v0.1.1) - 2026-07-07

### Fixed

- fixed focus true not working on non-first panes
