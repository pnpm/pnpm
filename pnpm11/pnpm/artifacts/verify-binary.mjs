#!/usr/bin/env node
// Prepublish gate for the @pnpm/<platform> artifact packages. Runs from the
// package directory (cwd contains the built pnpm binary). Verifies:
//   1. The binary exists with the expected filename for the target.
//   2. If the host can execute the target, `pnpm -v` returns a semver.
//
// Existence alone is not sufficient — @pnpm/exe@11.0.0-rc.4 shipped a binary
// that was present but crashed with a native SEA deserialization assertion on
// any invocation.
//
// Every published executable is executed on a host that can run it (via the
// release verification runners). When packaging from a host that cannot execute
// the target (e.g. musl or Windows on a macOS publish runner), skipping local
// execution is permitted only when the target is declared as covered by release
// verification (via PNPM_RELEASE_VERIFIED_TARGETS).
//
// Each platform package ships only the SEA binary (no dist/ or node_modules),
// but the SEA's CJS entry (pnpm.cjs) loads dist/pnpm.mjs from
// dirname(process.execPath). To run the binary in place we symlink
// ./dist -> ../exe/dist (the sibling @pnpm/exe package's staged bundle) for
// the duration of the test, then remove the symlink on exit. The platform
// package's "files" whitelist is "pnpm" only, so a stale symlink would never
// reach the published tarball, but we clean up anyway to leave the tree
// untouched for subsequent tools.
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
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
// host. Check if the target is declared covered by the release verification runners;
// otherwise fail immediately.
const osMatches = process.platform === targetOs
const archMatches = process.arch === targetArch
const libcMatches = targetOs !== 'linux' || !targetLibc || targetLibc === hostLibc

if (!osMatches || !archMatches || !libcMatches) {
  const targetLabel = [targetOs, targetArch, targetLibc].filter(Boolean).join('/')
  const targetKey = [targetOs, targetArch, targetLibc].filter(Boolean).join('-')
  const hostLabel = [process.platform, process.arch, hostLibc].filter(Boolean).join('/')

  const coveredTargets = (process.env.PNPM_RELEASE_VERIFIED_TARGETS ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)

  const isCovered =
    coveredTargets.includes(targetKey) ||
    coveredTargets.includes(targetLabel)

  if (isCovered) {
    console.log(`Skipping ${binName} -v: host ${hostLabel} cannot execute target ${targetLabel} (covered by release verification)`)
    process.exit(0)
  }

  console.error(`Error: host ${hostLabel} cannot execute target ${targetLabel}, and ${targetLabel} is not declared in PNPM_RELEASE_VERIFIED_TARGETS. Every published executable target must either execute on the current host or be covered by release verification runners.`)
  process.exit(1)
}

const distLinkPath = path.resolve('dist')
const distLinkTarget = path.join('..', 'exe', 'dist')
let distLinkCreated = false
// Remove a prior symlink from an aborted run so cleanup ownership is always
// well-defined. A real dist/ directory (unlikely in a platform package, but
// possible during development) is preserved — we treat it as external and
// skip cleanup.
try {
  if (fs.lstatSync(distLinkPath).isSymbolicLink()) fs.unlinkSync(distLinkPath)
} catch (err) {
  if (err.code !== 'ENOENT') throw err
}
const distPreexists = fs.existsSync(distLinkPath)

process.on('exit', () => {
  if (!distLinkCreated) return
  try { fs.unlinkSync(distLinkPath) } catch { /* nothing to clean up */ }
})

// Relocation check: before staging dist/, confirm the binary reads its bundle
// path from process.execPath at runtime and not from a build-time constant.
// A pnpm.cjs shim that accidentally captured __filename or a cwd-relative
// path during packaging would keep working on the build machine but break on
// every end-user machine. Asserting the error references the *runtime* cwd
// catches that regression here instead of after publish.
//
// Skipped when a real dist/ is already present (developer layout); in that
// case we can't distinguish a correctly-resolved dist from a hardcoded one.
if (!distPreexists) {
  const binPath = path.resolve(binName)
  const expectedRuntimeDist = path.join(fs.realpathSync(process.cwd()), 'dist', 'pnpm.mjs')
  let sansDistStdout
  try {
    sansDistStdout = execFileSync(binPath, ['-v'], {
      encoding: 'utf8',
      timeout: 30_000,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
  } catch (err) {
    const stderr = String(err?.stderr ?? '')
    // Expected: binary tried to require the runtime dist path and failed
    // because it isn't there. Anything else (a spawn error, crash signal,
    // timeout) is NOT evidence of a non-relocatable pnpm.cjs — it's an
    // unrelated failure that would hide the regression we actually care
    // about. Surface it with the raw diagnostic so the operator can tell
    // which one they're looking at.
    if (!stderr.includes(expectedRuntimeDist)) {
      const status = err?.status ?? 'none'
      const signal = err?.signal ?? 'none'
      const code = err?.code ?? 'none'
      console.error(`Error: ${binName} -v without dist/ did not fail with a missing-runtime-dist error. Either pnpm.cjs regressed to a non-relocatable form, or the binary failed for an unrelated reason. status=${status} signal=${signal} code=${code}\nstderr:\n${stderr}`)
      process.exit(1)
    }
  }
  if (sansDistStdout !== undefined) {
    console.error(`Error: ${binName} -v unexpectedly succeeded without dist/ alongside the binary. Output: ${JSON.stringify(sansDistStdout.trim())}. pnpm.cjs is loading a bundle from somewhere other than dirname(process.execPath); the published binary would ignore the dist shipped in @pnpm/exe.`)
    process.exit(1)
  }
}

// Only stage the symlink when nothing's there already. A junction is used on
// Windows so directory symlinks work without elevation / Developer Mode.
if (!distPreexists) {
  try {
    const resolvedTarget = path.resolve(distLinkTarget)
    fs.symlinkSync(resolvedTarget, distLinkPath, process.platform === 'win32' ? 'junction' : 'dir')
    distLinkCreated = true
  } catch (err) {
    console.error(`Error: could not stage dist/ symlink: ${String(err)}`)
    process.exit(1)
  }
}

let stdout
try {
  const binPath = path.resolve(binName)
  stdout = execFileSync(binPath, ['-v'], { encoding: 'utf8', timeout: 30_000 }).trim()
} catch (err) {
  console.error(`Error: ${binName} -v failed: ${String(err)}`)
  process.exit(1)
}

// Accept SemVer 2 with optional prerelease and build-metadata suffixes so a
// future `11.0.0-rc.4+sha.<hash>` release doesn't fail this gate spuriously.
if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?(?:\+[\w.-]+)?$/.test(stdout)) {
  console.error(`Error: ${binName} -v produced unexpected output: ${JSON.stringify(stdout)}`)
  process.exit(1)
}

console.log(`${binName} -v OK (${stdout})`)
