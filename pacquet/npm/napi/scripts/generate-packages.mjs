// Adapted from `pacquet/npm/pnpm/scripts/generate-packages.mjs`.
//
// Generates the per-platform `@pnpm/napi.<target>` native packages (each
// carrying one prebuilt `.node` addon) and patches the `@pnpm/napi`
// wrapper's `optionalDependencies` to reference them. CI cross-compiles the
// addon per target (`napi build --target <rust-triple>`), uploads the artifacts
// as `pnpm-napi.<codeTarget>.node` at the repo root, then runs this.
//
// The wrapper's `index.js` resolves `@pnpm/napi.<triple>` at load time,
// where `<triple>` is `${process.platform}-${process.arch}` (plus `-musl` on
// musl Linux) — the `packageTarget` values below. Each native package's `main`
// points at its `.node`, so `require('@pnpm/napi.<triple>')` returns the
// addon.

import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import * as fs from 'node:fs'

// Base name of the cross-compiled artifacts CI uploads at the repo root.
const ARTIFACT_BASE = 'pnpm-napi'
// The `.node` file name inside each native package (also the package `main`).
const NATIVE_ADDON_FILE = 'pnpm-napi.node'

const WRAPPER_ROOT = resolve(fileURLToPath(import.meta.url), '../..')
const PACKAGES_ROOT = resolve(WRAPPER_ROOT, '..')
const REPO_ROOT = resolve(PACKAGES_ROOT, '../..')
const MANIFEST_PATH = resolve(WRAPPER_ROOT, 'package.json')

const rootManifest = JSON.parse(fs.readFileSync(MANIFEST_PATH, 'utf-8'))

// One entry per prebuilt target. `packageTarget` must match the triple the
// wrapper's `platformTriple()` computes at runtime.
const TARGETS = [
  { platform: 'win32', arch: 'x64', codeTarget: 'win32-x64', packageTarget: 'win32-x64' },
  { platform: 'win32', arch: 'arm64', codeTarget: 'win32-arm64', packageTarget: 'win32-arm64' },
  { platform: 'darwin', arch: 'x64', codeTarget: 'darwin-x64', packageTarget: 'darwin-x64' },
  { platform: 'darwin', arch: 'arm64', codeTarget: 'darwin-arm64', packageTarget: 'darwin-arm64' },
  { platform: 'linux', arch: 'x64', libc: 'glibc', codeTarget: 'linux-x64', packageTarget: 'linux-x64' },
  { platform: 'linux', arch: 'arm64', libc: 'glibc', codeTarget: 'linux-arm64', packageTarget: 'linux-arm64' },
  { platform: 'linux', arch: 'x64', libc: 'musl', codeTarget: 'linux-x64-musl', packageTarget: 'linux-x64-musl' },
  { platform: 'linux', arch: 'arm64', libc: 'musl', codeTarget: 'linux-arm64-musl', packageTarget: 'linux-arm64-musl' },
]

function nativePackageName(target) {
  return `@pnpm/napi.${target.packageTarget}`
}

function generateNativePackage(target) {
  const packageName = nativePackageName(target)
  const packageRoot = resolve(PACKAGES_ROOT, `napi.${target.codeTarget}`)
  fs.rmSync(packageRoot, { recursive: true, force: true })
  fs.mkdirSync(packageRoot)

  const { version } = rootManifest
  const manifestData = {
    name: packageName,
    version,
    description: `Prebuilt pnpm v12 Rust engine addon for ${target.packageTarget}`,
    license: 'MIT',
    os: [target.platform],
    cpu: [target.arch],
    main: NATIVE_ADDON_FILE,
    files: [NATIVE_ADDON_FILE],
    repository: { type: 'git', url: 'https://github.com/pnpm/pnpm' },
  }
  if (target.libc) {
    manifestData.libc = [target.libc]
  }
  fs.writeFileSync(resolve(packageRoot, 'package.json'), `${JSON.stringify(manifestData, null, 2)}\n`)

  const source = resolve(REPO_ROOT, `${ARTIFACT_BASE}.${target.codeTarget}.node`)
  if (!fs.existsSync(source)) {
    console.warn(`WARN: missing prebuilt artifact ${source}; skipping ${packageName}`)
    return false
  }
  fs.copyFileSync(source, resolve(packageRoot, NATIVE_ADDON_FILE))
  console.log(`Generated ${packageName}`)
  return true
}

function patchWrapperOptionalDependencies(generatedTargets) {
  const optionalDependencies = Object.fromEntries(
    generatedTargets.map((target) => [nativePackageName(target), rootManifest.version])
  )
  rootManifest.optionalDependencies = {
    ...rootManifest.optionalDependencies,
    ...optionalDependencies,
  }
  fs.writeFileSync(MANIFEST_PATH, `${JSON.stringify(rootManifest, null, 2)}\n`)
  console.log('Patched @pnpm/napi optionalDependencies')
}

const generatedTargets = TARGETS.filter((target) => generateNativePackage(target))
patchWrapperOptionalDependencies(generatedTargets)
