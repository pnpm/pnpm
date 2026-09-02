// Where the host's native pnpm binary comes from. Shared by the preinstall
// (`install.js`), which links it over the placeholder bins, and by the Corepack
// entry (`bin/pnpm.mjs`), which spawns it.
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const { platform, arch } = process

/** Directory of the published wrapper; the native binary lives next to it. */
export const wrapperDir = path.dirname(fileURLToPath(import.meta.url))

const PLATFORMS = {
  win32: {
    x64: '@pnpm/exe.win32-x64/pnpm.exe',
    arm64: '@pnpm/exe.win32-arm64/pnpm.exe',
  },
  darwin: {
    x64: '@pnpm/exe.darwin-x64/pnpm',
    arm64: '@pnpm/exe.darwin-arm64/pnpm',
  },
  linux: {
    x64: {
      glibc: '@pnpm/exe.linux-x64/pnpm',
      musl: '@pnpm/exe.linux-x64-musl/pnpm',
    },
    arm64: {
      glibc: '@pnpm/exe.linux-arm64/pnpm',
      musl: '@pnpm/exe.linux-arm64-musl/pnpm',
    },
  },
}

/**
 * Native binary specifiers to try, most-preferred first; empty when the host is
 * unsupported. The linux glibc/musl pair is ordered by detected libc, which
 * only decides the winner when both are installed (e.g. `npm install --force`).
 *
 * @returns {string[]}
 */
export function getBinCandidates () {
  const platformEntry = PLATFORMS?.[platform]?.[arch]

  if (platformEntry == null) {
    return []
  }
  if (typeof platformEntry === 'string') {
    return [platformEntry]
  }

  const order = detectLinuxLibc() === 'musl' ? ['musl', 'glibc'] : ['glibc', 'musl']
  return order.map((libc) => platformEntry[libc])
}

/**
 * Split a specifier from {@link getBinCandidates} into the package that ships
 * the binary and the binary's path inside it.
 *
 * @param {string} specifier
 * @returns {{ packageName: string, binFile: string }}
 */
export function splitBinSpecifier (specifier) {
  const [scope, name, ...rest] = specifier.split('/')
  return { packageName: `${scope}/${name}`, binFile: rest.join('/') }
}

/**
 * Path to the native binary of whichever platform package the package manager
 * installed with this wrapper, or `null` when none is present (Corepack,
 * `--no-optional`).
 *
 * Only the wrapper's own `node_modules` and the one it was installed into are
 * searched, which is where every package manager puts an optional dependency
 * of the wrapper. A `require` walk would also reach the ancestors' `node_modules`,
 * which belong to whatever project the wrapper sits under and are not to be
 * run in its name.
 *
 * @returns {string | null}
 */
export function resolveInstalledBinary () {
  // Use whichever platform package the package manager installed: it already
  // filtered by `os`/`cpu`/`libc`, more reliable than re-deriving the host.
  for (const specifier of getBinCandidates()) {
    const { packageName, binFile } = splitBinSpecifier(specifier)
    for (const modulesDir of installedModulesDirs()) {
      const candidate = path.join(modulesDir, ...packageName.split('/'), binFile)
      if (fs.statSync(candidate, { throwIfNoEntry: false })?.isFile() === true) {
        return candidate
      }
    }
  }
  return null
}

/**
 * Where a platform package installed for this wrapper can be: the `node_modules`
 * nested in the wrapper, then the one the wrapper was installed into (two levels
 * up for the scoped `@pnpm/exe`). Neither need exist. Throws only when the
 * wrapper's manifest is unreadable, see {@link readWrapperManifest}.
 *
 * @returns {string[]}
 */
function installedModulesDirs () {
  const { name } = readWrapperManifest()
  const segments = typeof name === 'string' ? name.split('/').length : 1
  return [
    path.join(wrapperDir, 'node_modules'),
    path.resolve(wrapperDir, ...Array(segments).fill('..')),
  ]
}

// A successful read with no optionalDependencies is the dev checkout (there is
// no native binary to link there); a read/parse failure is a corrupt published
// package and must not be silently swallowed into that same path.
export function readWrapperManifest () {
  const manifestPath = path.join(wrapperDir, 'package.json')
  let manifest
  try {
    manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
  } catch (err) {
    throw new Error(`Failed to read ${manifestPath}: ${err.message}`)
  }
  if (typeof manifest !== 'object' || manifest == null || Array.isArray(manifest)) {
    throw new Error(`Expected ${manifestPath} to contain a JSON object`)
  }
  return manifest
}

function detectLinuxLibc () {
  if (platform !== 'linux') {
    return null
  }

  // glibc builds set `glibcVersionRuntime`; musl leaves it unset. Guarded —
  // `process.report` may be unavailable, leaving ordering to the default.
  try {
    return process.report?.getReport().header.glibcVersionRuntime ? 'glibc' : 'musl'
  } catch {
    return null
  }
}
