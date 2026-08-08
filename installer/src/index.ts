import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { familySync } from 'detect-libc'

import { extractTarballMember } from './extractTarball.js'
import { platformPackageName } from './platformPackageName.js'
import { downloadTarball, fetchPackument, fetchVersionMeta, registryFromEnv } from './registry.js'
import { resolveVersion } from './resolveVersion.js'

export { platformPackageName, type Target } from './platformPackageName.js'
export { DEFAULT_REGISTRY, registryFromEnv } from './registry.js'
export { type Packument, resolveVersion } from './resolveVersion.js'

/**
 * The package that carries the version line of every pnpm release and, from
 * v11 on, the `dist/` tree that ships next to the executable.
 */
const WRAPPER_PKG_NAME = '@pnpm/exe'

const USAGE = `Usage: npx get-pnpm [version]

Installs pnpm as a standalone executable and adds it to your PATH.

Arguments:
  version            An exact version (11.20.0), a major (12), or a dist-tag
                     (latest, next-12). Defaults to $PNPM_VERSION, then "latest".

Environment variables:
  PNPM_VERSION           Version to install when no argument is given.
  PNPM_HOME              Directory to install pnpm into.
  npm_config_registry    Registry to download pnpm from.
`

export async function runCli (argv: string[]): Promise<number> {
  const positional: string[] = []
  for (const arg of argv) {
    if (arg === '--help' || arg === '-h') {
      console.log(USAGE)
      return 0
    }
    if (arg.startsWith('-')) {
      throw new Error(`Unknown option "${arg}".\n\n${USAGE}`)
    }
    positional.push(arg)
  }
  if (positional.length > 1) {
    throw new Error(`Expected at most one version, got ${positional.length}.\n\n${USAGE}`)
  }
  return installPnpm({
    versionSpec: positional[0] ?? process.env.PNPM_VERSION ?? 'latest',
    registry: registryFromEnv(),
  })
}

/**
 * Downloads the pnpm executable and hands over to `pnpm setup`, which installs
 * it globally and puts it on the PATH.
 *
 * The temporary directory is assembled to look exactly like the release tarball
 * that https://get.pnpm.io/install.sh downloads — the executable next to the
 * `dist/` tree — because `pnpm setup` installs that directory as-is.
 *
 * @returns the exit code of `pnpm setup`.
 */
export async function installPnpm (opts: { versionSpec: string, registry: string }): Promise<number> {
  const packument = await fetchPackument(opts.registry, WRAPPER_PKG_NAME)
  const version = resolveVersion(packument, opts.versionSpec)
  const major = majorVersion(version)
  const platformPkgName = platformPackageName({
    major,
    platform: process.platform,
    arch: process.arch,
    libcFamily: familySync(),
  })

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-install-'))
  const removeTmpDir = (): void => {
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) {
    process.once(signal, () => {
      removeTmpDir()
      process.exit(1)
    })
  }
  try {
    console.log(`==> Downloading pnpm ${version}`)
    const executable = process.platform === 'win32' ? 'pnpm.exe' : 'pnpm'
    const downloadInto = tarballDownloader({ dir: tmpDir, registry: opts.registry, version })
    const downloads = [downloadInto(platformPkgName, `package/${executable}`)]
    // v11 was the first release to keep files next to the executable; up to
    // v10 the executable is self-contained and `@pnpm/exe` ships no `dist/`.
    if (major >= 11) {
      downloads.push(downloadInto(WRAPPER_PKG_NAME, 'package/dist'))
    }
    await Promise.all(downloads)
    const binPath = path.join(tmpDir, executable)
    fs.chmodSync(binPath, 0o755)

    const { error, status } = spawnSync(binPath, ['setup', '--force'], { stdio: 'inherit' })
    if (error != null) throw error
    return status ?? 1
  } finally {
    removeTmpDir()
  }
}

/** Downloads packages of one version into one directory, keeping `member` of each. */
function tarballDownloader (
  opts: { dir: string, registry: string, version: string }
): (pkgName: string, member: string) => Promise<void> {
  return async function downloadInto (pkgName: string, member: string): Promise<void> {
    const meta = await fetchVersionMeta(opts.registry, pkgName, opts.version)
    const tarball = path.join(opts.dir, `${pkgName.replaceAll('/', '-')}.tgz`)
    await downloadTarball(meta, tarball)
    extractTarballMember(tarball, opts.dir, member)
    fs.rmSync(tarball)
  }
}

function majorVersion (version: string): number {
  const major = Number(version.split('.')[0])
  if (!Number.isInteger(major)) {
    throw new Error(`Could not read a major version from "${version}".`)
  }
  return major
}
