# Changelog

## 0.2.0 (2026-10-09)


### Bug Fixes

* **kotor-ncs:** keep initialisers when a sub's result feeds an expression ([54b90b0](https://github.com/kolto-labs/kotor-formats/commit/54b90b053edb8bcaa9c1799fc84b38900a7e57ab))

## Changelog

## Unreleased

### Features

* NCS bytecode reader and writer, plus a DeNCS-algorithm decompiler to NSS.
* The decompiler takes its engine-function signatures from a caller-supplied `ActionTable` (`ActionTable::from_nwscript` parses an install's `nwscript.nss`). No engine-function data is compiled in.
