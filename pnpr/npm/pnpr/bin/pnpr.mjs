#!/usr/bin/env node
import { spawnSync } from 'node:child_process'
import console from 'node:console'
import os from 'node:os'
import process from 'node:process'
import { resolveInstalledBinary } from '../install.js'

const binary = resolveInstalledBinary()
if (binary == null) {
  console.error('@pnpm/pnpr has no native binary installed for this platform.')
  console.error('If your package manager skipped optional dependencies, enable them and reinstall.')
  process.exit(1)
}

if (process.stderr.isTTY) {
  console.error('pnpr is running through Node.js fallback because install scripts were skipped.')
  console.error('Reinstall @pnpm/pnpr with its build scripts allowed to run the binary directly.')
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' })
if (result.error != null) {
  console.error(`Could not run pnpr binary at ${binary}: ${result.error.message}`)
  process.exit(1)
}
process.exit(
  result.signal == null
    ? (result.status ?? 1)
    : 128 + (os.constants.signals[result.signal] ?? 0)
)
