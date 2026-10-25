import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { extractTarball } from './extractTarball.js'
import { registryConfigFromEnv } from './npmConfig.js'
import { isMusl, platformPackageName } from './platformPackageName.js'
import { readProjectPin } from './projectPin.js'
import { downloadTarball, fetchPackument, fetchSignatures, fetchVersionMeta, type RequestHeaders, useCertificateAuthorities, useProxyFromEnv } from './registry.js'
import { majorVersion, resolveVersion } from './resolveVersion.js'
import { findShadowingPnpm, pnpmHomeDir, renderShadowingPnpmWarning } from './shadowingPnpm.js'
import { renderUndoHint } from './undoHint.js'
import { type SigningKey, verifyRegistrySignature } from './verifySignature.js'

/**
 * The package whose dist-tags name every pnpm release. `@pnpm/exe` carries the
 * same versions today, but only `pnpm` is published from v12 onward, so its
 * tags are the ones that cannot go stale.
 */
const CLI_PKG_NAME = 'pnpm'

/** Holds the unpacked tarballs while the installation is assembled beside it. */
const UNPACK_DIR = '.unpack'

/** Where the `dist/` tree that ships beside the executable is published. */
function wrapperPackageName (major: number): string {
  return major >= 12 ? CLI_PKG_NAME : '@pnpm/exe'
}

const USAGE = `Usage: npx get-pnpm [version]

Installs pnpm as a standalone executable and adds it to your PATH.

Arguments:
  version            An exact version (11.20.0), a major (12), a semver range
                     (^12.1.0), or a dist-tag (latest, next-12). Defaults to
                     $PNPM_VERSION, then the version the project in the current
                     directory pins, then "latest".

                     The project's pin is read from devEngines.packageManager
                     or packageManager in its manifest, or the workspace
                     root's for a workspace project, the way pnpm reads it. A
                     range resolves to the version pnpm-lock.yaml records
                     while that is still in range.

Environment variables:
  PNPM_VERSION           Version to install when no argument is given.
  PNPM_HOME              Directory to install pnpm into.
  npm_config_registry    Registry to download pnpm from.
  npm_config_*:_authToken
                         Registry-scoped bearer token. Project, user, and
                         global .npmrc auth tokens are also read.
  npm_config_ca, npm_config_cafile
                         CA certificates to trust for the registry, besides
                         Node.js's own. Project, user, and global .npmrc ca,
                         ca[], and cafile settings, top-level or scoped to the
                         registry, are also read. Node 22.19/24.5 or later.
  HTTPS_PROXY            Proxy to download through (HTTP_PROXY for an http://
                         registry); NO_PROXY names hosts to reach directly.
                         Node 24.14 or later.
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
  const registryConfig = registryConfigFromEnv()
  return installPnpm({
    versionSpec: positional[0] ?? process.env.PNPM_VERSION ?? projectVersionSpec(process.cwd()) ?? 'latest',
    ...registryConfig,
  })
}

/** The version `readProjectPin` finds for `dir`, saying where it comes from. */
function projectVersionSpec (dir: string): string | undefined {
  const pin = readProjectPin(dir)
  if (pin == null) return undefined
  const locked = pin.lockedVersion == null ? '' : `, locked to ${pin.lockedVersion} in pnpm-lock.yaml`
  console.log(`==> ${pin.manifestPath} pins pnpm ${pin.spec} in ${pin.field}${locked}`)
  return pin.lockedVersion ?? pin.spec
}

/**
 * Downloads the pnpm executable and hands over to `pnpm setup`, which installs
 * it globally and puts it on the PATH.
 *
 * Every download is checked against the checksum the registry published for it,
 * and that checksum against npm's signature — see `verifyRegistrySignature`.
 *
 * The temporary directory is assembled to look like the release tarball that
 * https://get.pnpm.io/install.sh downloads — the executable next to its `dist/`
 * tree — because `pnpm setup` installs that directory as-is.
 *
 * @returns the exit code of `pnpm setup`.
 */
export async function installPnpm (
  opts: {
    versionSpec: string
    registry: string
    authPath?: string
    headers?: RequestHeaders
    ca?: readonly string[]
    keys?: readonly SigningKey[]
  }
): Promise<number> {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-install-'))
  const removeTmpDir = (): void => {
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
  // Registered for the duration of the call and no longer: this is also a
  // library function, and handlers left behind would accumulate and outlive the
  // directory they exist to clean up.
  const signals = ['SIGINT', 'SIGTERM', 'SIGHUP'] as const
  const onSignal = (): void => {
    removeTmpDir()
    // eslint-disable-next-line n/no-process-exit -- a signal listener replaces the default handling, which would have ended the process
    process.exit(1)
  }
  for (const signal of signals) {
    process.once(signal, onSignal)
  }
  try {
    const { binPath } = await downloadPnpm({ ...opts, dest: tmpDir })
    const { error, status } = spawnSync(binPath, ['setup', '--force'], { stdio: 'inherit' })
    if (error != null) throw error
    if (status === 0) {
      console.log(renderUndoHint(pnpmHomeDir()))
      warnIfPnpmIsShadowed()
    }
    return status ?? 1
  } finally {
    for (const signal of signals) {
      process.off(signal, onSignal)
    }
    removeTmpDir()
  }
}

/**
 * Says so when the pnpm `pnpm setup` just installed is not the one PATH
 * resolves, or the success above is the last thing the person reads before
 * `pnpm --version` prints the old version again.
 */
function warnIfPnpmIsShadowed (): void {
  const pnpmHome = pnpmHomeDir()
  const shadowing = findShadowingPnpm(pnpmHome, { pathEnv: readPathEnv(process.env) })
  if (shadowing != null) {
    console.error(renderShadowingPnpmWarning(shadowing, pnpmHome))
  }
}

/** Windows spells the variable `Path`, and the environment is case-insensitive there. */
function readPathEnv (env: NodeJS.ProcessEnv): string | undefined {
  if (process.platform !== 'win32') return env.PATH
  const key = Object.keys(env).find((name) => name.toUpperCase() === 'PATH')
  return key != null ? env[key] : undefined
}

/**
 * Downloads the pnpm executable into `dest`, laid out the way the release
 * tarball lays it out — the executable next to the `dist/` tree it loads.
 *
 * Every download is checked against the checksum the registry published for it,
 * and that checksum against npm's signature; see `verifyRegistrySignature`.
 * Nothing outside `dest` is touched, so a caller that manages its own PATH — a
 * CI action, say — can use this without the global install `installPnpm` does.
 *
 * @returns the version installed and the path to the executable.
 */
export async function downloadPnpm (
  opts: {
    versionSpec: string
    registry: string
    dest: string
    authPath?: string
    headers?: RequestHeaders
    /** PEM certificates of CAs to trust besides Node.js's own; see {@link useCertificateAuthorities}. */
    ca?: readonly string[]
    keys?: readonly SigningKey[]
  }
): Promise<{ version: string, binPath: string }> {
  const restoreProxy = useProxyFromEnv()
  try {
    const restoreCertificates = useCertificateAuthorities(opts.ca)
    try {
      return await fetchPnpm(opts)
    } finally {
      restoreCertificates()
    }
  } finally {
    restoreProxy()
  }
}

async function fetchPnpm (
  opts: {
    versionSpec: string
    registry: string
    dest: string
    authPath?: string
    headers?: RequestHeaders
    keys?: readonly SigningKey[]
  }
): Promise<{ version: string, binPath: string }> {
  const packument = await fetchPackument(opts.registry, CLI_PKG_NAME, opts.headers, opts.authPath)
  const version = resolveVersion(packument, opts.versionSpec)
  const major = majorVersion(version)
  const platformPkgName = platformPackageName({
    major,
    platform: process.platform,
    arch: process.arch,
    musl: isMusl(),
  })

  const { dest } = opts
  fs.mkdirSync(dest, { recursive: true })
  try {
    console.log(`==> Downloading pnpm ${version}`)
    const executable = process.platform === 'win32' ? 'pnpm.exe' : 'pnpm'
    const fetchPackage = verifiedPackageFetcher({
      dir: dest,
      registry: opts.registry,
      version,
      authPath: opts.authPath,
      headers: opts.headers,
      keys: opts.keys,
    })

    const { platformPkg, wrapperPkg } = await fetchPackages(fetchPackage, platformPkgName, major)
    const binPath = path.join(dest, executable)
    fs.renameSync(path.join(platformPkg.dir, executable), binPath)
    fs.chmodSync(binPath, 0o755)
    if (wrapperPkg != null) {
      fs.rmSync(path.join(dest, 'dist'), { recursive: true, force: true })
      fs.renameSync(path.join(wrapperPkg.dir, 'dist'), path.join(dest, 'dist'))
      writeManifest({ dest, executable, version, wrapperDir: wrapperPkg.dir, major })
    }
    return { version, binPath }
  } finally {
    // `pnpm setup` installs this directory as a package, so nothing may be left
    // in it that does not belong in the installation.
    fs.rmSync(path.join(dest, UNPACK_DIR), { recursive: true, force: true })
  }
}

async function fetchPackages (
  fetchPackage: (pkgName: string) => Promise<{ dir: string }>,
  platformPkgName: string,
  major: number
): Promise<{ platformPkg: { dir: string }, wrapperPkg: { dir: string } | undefined }> {
  // Settled, not `all`: a rejection there would leave the other fetch writing
  // into the directory the caller is about to remove.
  const [platformResult, wrapperResult] = await Promise.allSettled([
    fetchPackage(platformPkgName),
    // v11 was the first release to keep files next to the executable; up to
    // v10 the executable is self-contained and ships no `dist/`.
    major >= 11 ? fetchPackage(wrapperPackageName(major)) : Promise.resolve(undefined),
  ])
  if (platformResult.status === 'rejected') throw platformResult.reason
  if (wrapperResult.status === 'rejected') throw wrapperResult.reason
  return { platformPkg: platformResult.value, wrapperPkg: wrapperResult.value }
}

/**
 * `pnpm setup` installs the directory as a package, writing a minimal manifest
 * when there is none — which is what the release tarball relies on. That
 * tarball bundles the runtime dependencies inside `dist/`; the registry copy
 * declares them instead, so up to v11 they have to be declared here or the
 * install silently loses them (`@reflink/reflink`, and with it copy-on-write
 * cloning). From v12 the `dist/` tree is self-contained again, so the manifest
 * `setup` writes is left to it.
 */
function writeManifest (
  opts: { dest: string, executable: string, version: string, wrapperDir: string, major: number }
): void {
  if (opts.major >= 12) return
  const wrapper = JSON.parse(fs.readFileSync(path.join(opts.wrapperDir, 'package.json'), 'utf8')) as { dependencies?: Record<string, string> }
  if (wrapper.dependencies == null) return
  fs.writeFileSync(path.join(opts.dest, 'package.json'), JSON.stringify({
    name: '@pnpm/exe',
    version: opts.version,
    type: 'module',
    bin: { pnpm: opts.executable, pn: opts.executable },
    dependencies: wrapper.dependencies,
  }))
}

/** Downloads packages of one version into one directory, verifying each. */
function verifiedPackageFetcher (
  opts: {
    dir: string
    registry: string
    version: string
    authPath?: string
    headers?: RequestHeaders
    keys?: readonly SigningKey[]
  }
): (pkgName: string) => Promise<{ dir: string }> {
  return async function fetchPackage (pkgName: string): Promise<{ dir: string }> {
    const meta = await fetchVersionMeta(opts.registry, pkgName, opts.version, opts.headers, opts.authPath)
    if (!meta.dist.integrity) {
      throw new Error(`The npm registry published no checksum for ${pkgName}@${opts.version}, so it cannot be verified.`)
    }
    verifyRegistrySignature({
      name: pkgName,
      version: opts.version,
      integrity: meta.dist.integrity,
      signatures: await fetchSignatures(meta, {
        registry: opts.registry,
        pkgName,
        version: opts.version,
        headers: opts.headers,
        headersPath: opts.authPath,
      }),
      keys: opts.keys,
    })
    // Unpack away from the directory being assembled: `pnpm` is both a package
    // name and the name of the executable that ends up beside it.
    const unpackDir = path.join(opts.dir, UNPACK_DIR, pkgName.replaceAll('/', '-'))
    const tarball = `${unpackDir}.tgz`
    fs.mkdirSync(path.dirname(tarball), { recursive: true })
    await downloadTarball(meta, tarball, {
      registry: opts.registry,
      headers: opts.headers,
      headersPath: opts.authPath,
    })
    extractTarball(tarball, unpackDir)
    fs.rmSync(tarball)
    return { dir: path.join(unpackDir, 'package') }
  }
}
