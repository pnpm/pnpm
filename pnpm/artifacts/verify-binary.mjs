#!/usr/bin/env node
// Prepublish gate for the @pnpm/<platform> artifact packages. Runs from the
// package directory (cwd contains the built pnpm binary). Verifies:
//   1. The binary exists with the expected filename for the target.
//   2. If the host can execute the target, `pnpm -v` returns a semver.
//
// Existence alone is not sufficient — @pnpm/exe@11.0.0-rc.4 shipped a binary
// that was present but crashed with a native SEA deserialization assertion on
// any invocation. Executing -v would have caught it on the Linux CI host.
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'

const [targetOs, targetArch] = process.argv.slice(2)
if (!targetOs || !targetArch) {
  console.error('Usage: verify-binary.mjs <os> <arch>')
  process.exit(2)
}

const binName = targetOs === 'win32' ? 'pnpm.exe' : 'pnpm'
if (!fs.existsSync(binName)) {
  console.error(`Error: ${binName} is missing in ${process.cwd()}`)
  process.exit(1)
}

// Cross-platform targets (e.g. win32 or darwin from a Linux CI) can't be
// executed from the publish host. Existence is the best we can verify.
if (process.platform !== targetOs || process.arch !== targetArch) {
  console.log(`Skipping ${binName} -v: host ${process.platform}/${process.arch} cannot execute target ${targetOs}/${targetArch}`)
  process.exit(0)
}

let stdout
try {
  stdout = execFileSync(`./${binName}`, ['-v'], { encoding: 'utf8', timeout: 30_000 }).trim()
} catch (err) {
  console.error(`Error: ${binName} -v failed: ${err.message}`)
  process.exit(1)
}

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(stdout)) {
  console.error(`Error: ${binName} -v produced unexpected output: ${JSON.stringify(stdout)}`)
  process.exit(1)
}

console.log(`${binName} -v OK (${stdout})`)
