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
import process from 'node:process'

const [targetOs, targetArch, targetLibc] = process.argv.slice(2)
if (!targetOs || !targetArch) {
  console.error('Usage: verify-binary.mjs <os> <arch> [libc]')
  process.exit(2)
}

const binName = targetOs === 'win32' ? 'pnpm.exe' : 'pnpm'
if (!fs.existsSync(binName)) {
  console.error(`Error: ${binName} is missing in ${process.cwd()}`)
  process.exit(1)
}

// Node populates header.glibcVersionRuntime only on glibc hosts, so its
// presence is a reliable glibc/musl discriminator without shelling out.
function detectHostLibc () {
  if (process.platform !== 'linux') return null
  const header = process.report.getReport().header
  return header.glibcVersionRuntime ? 'glibc' : 'musl'
}
const hostLibc = detectHostLibc()

// Cross-platform or cross-libc targets can't be executed from the publish
// host. Existence is the best we can verify — skip the -v check instead of
// failing, so a musl artifact published from a glibc CI still goes through.
const osMatches = process.platform === targetOs
const archMatches = process.arch === targetArch
const libcMatches = targetOs !== 'linux' || !targetLibc || targetLibc === hostLibc

if (!osMatches || !archMatches || !libcMatches) {
  const targetLabel = [targetOs, targetArch, targetLibc].filter(Boolean).join('/')
  const hostLabel = [process.platform, process.arch, hostLibc].filter(Boolean).join('/')
  console.log(`Skipping ${binName} -v: host ${hostLabel} cannot execute target ${targetLabel}`)
  process.exit(0)
}

let stdout
try {
  stdout = execFileSync(`./${binName}`, ['-v'], { encoding: 'utf8', timeout: 30_000 }).trim()
} catch (err) {
  console.error(`Error: ${binName} -v failed: ${err.message}`)
  process.exit(1)
}

if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(stdout)) {
  console.error(`Error: ${binName} -v produced unexpected output: ${JSON.stringify(stdout)}`)
  process.exit(1)
}

console.log(`${binName} -v OK (${stdout})`)
