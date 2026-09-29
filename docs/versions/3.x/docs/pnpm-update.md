---
id: pnpm-update
title: pnpm update
original_id: pnpm-update
---

`pnpm update` updates packages to their latest version based on the specified range.

## tl;dr

|Command|Meaning|
|--|--|
|`pnpm up` |updates all dependencies. Adheres ranges specified in `package.json` |
|`pnpm up --latest` |updates all dependencies. Ignores ranges specified in `package.json` |
|`pnpm up foo@2` |updates `foo` to the latest v2 |

## Options

### --latest

Added in: v3.2.0

Ignores the version range specified in `package.json`. Instead, the version specified by the `latest` tag will be used (potentially upgrading the packages across major versions).
