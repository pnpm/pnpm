#!/usr/bin/env node
'use strict'

// Copy the cargo-built cdylib to a predictable name next to index.js.
// napi-rs crates produce libpnpm_cafs_writer.{dylib,so,dll}; we rename to
// pnpm-cafs-writer.<platform>-<arch>.node which is what index.js looks for.

const { copyFileSync, existsSync } = require('node:fs')
const { join } = require('node:path')

const debug = process.argv.includes('--debug')
const profile = debug ? 'debug' : 'release'
const root = join(__dirname, '..')

const sourceByPlatform = {
  darwin: 'libpnpm_cafs_writer.dylib',
  linux: 'libpnpm_cafs_writer.so',
  win32: 'pnpm_cafs_writer.dll',
}

const sourceName = sourceByPlatform[process.platform]
if (!sourceName) {
  console.error(`Unsupported platform: ${process.platform}`)
  process.exit(1)
}

const source = join(root, 'target', profile, sourceName)
if (!existsSync(source)) {
  console.error(`Expected cargo artifact not found: ${source}`)
  process.exit(1)
}

const triple = `${process.platform}-${process.arch}`
const dest = join(root, `pnpm-cafs-writer.${triple}.node`)
copyFileSync(source, dest)
console.log(`Wrote ${dest}`)
