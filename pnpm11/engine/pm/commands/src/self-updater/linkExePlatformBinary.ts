import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { familySync } from 'detect-libc'
import semver from 'semver'

/**
 * Computes the scope-local directory name of the `@pnpm/exe` platform
 * package for a given host. Returns the legacy name currently published on npm
 * (`macos-<arch>`, `win-<arch>`, `linux-<arch>`, `linuxstatic-<arch>`); callers
 * should also consider the future `exe.<platform>-<arch>[-musl]` scheme, since
 * a later release will switch to it. Pure so that the musl branch is
 * unit-testable without mocking detect-libc or patching process.platform.
 */
export function exePlatformPkgDirName (
  platform: NodeJS.Platform,
  arch: string,
  libcFamily: string | null
): string {
  const normalizedArch = platform === 'win32' && arch === 'ia32' ? 'x86' : arch
  return `${legacyOsSegment(platform, libcFamily)}-${normalizedArch}`
}

function legacyOsSegment (platform: NodeJS.Platform, libcFamily: string | null): string {
  switch (platform) {
    case 'darwin': return 'macos'
    case 'win32': return 'win'
    case 'linux': return libcFamily === 'musl' ? 'linuxstatic' : 'linux'
    default: return platform
  }
}

/**
 * Scope-local directory name of the platform package under the
 * `exe.<platform>-<arch>[-musl]` scheme, i.e. the published package
 * `@pnpm/exe.<platform>-<arch>[-musl]`. pnpm v12 (the Rust port) ships its
 * native binaries under exactly this convention, so `linkExePlatformBinary`
 * relinks a v12 install with no v12-specific logic. `@pnpm/exe` (the
 * TypeScript SEA build) is expected to adopt the same scheme in a future
 * release, which is why the legacy `@pnpm/<os>-<arch>` name is still checked
 * first as a fallback.
 */
export function exePlatformPkgDirNameNext (
  platform: NodeJS.Platform,
  arch: string,
  libcFamily: string | null
): string {
  return `exe.${nativeTargetName(platform, arch, libcFamily)}`
}

/**
 * The `<platform>-<arch>[-musl]` target a pnpm native binary is built for,
 * as the `exe.<target>` platform packages are named after it.
 */
export function nativeTargetName (
  platform: NodeJS.Platform,
  arch: string,
  libcFamily: string | null
): string {
  const normalizedArch = platform === 'win32' && arch === 'ia32' ? 'x86' : arch
  const libcSuffix = platform === 'linux' && libcFamily === 'musl' ? '-musl' : ''
  return `${platform}-${normalizedArch}${libcSuffix}`
}

// The wrapper's preinstall links the platform binary into the wrapper dir, but
// scripts are disabled during pnpm's own installs, so replicate it here — trying
// the legacy and the newer `exe.<target>` platform-package names.
export function linkExePlatformBinary (installDir: string, wrapperPkgName: string = '@pnpm/exe'): void {
  const wrapperDir = path.join(installDir, 'node_modules', ...wrapperPkgName.split('/'))
  if (!fs.existsSync(wrapperDir)) return
  const executable = process.platform === 'win32' ? 'pnpm.exe' : 'pnpm'
  const src = findPlatformBinary({ installDir, wrapperDir, wrapperPkgName, executable })
  if (src == null) return
  forceLink(src, path.join(wrapperDir, executable))

  if (process.platform === 'win32') {
    linkWindowsAliases(src, wrapperDir)
  } else if (isRustWrapper(wrapperDir, wrapperPkgName)) {
    for (const alias of ['pn', 'pnpx', 'pnx']) forceLink(src, path.join(wrapperDir, alias))
  }
}

function isRustWrapper (wrapperDir: string, wrapperPkgName: string): boolean {
  if (wrapperPkgName !== 'pnpm' && wrapperPkgName !== '@pnpm/exe') return false
  const { version } = JSON.parse(fs.readFileSync(path.join(wrapperDir, 'package.json'), 'utf8'))
  return typeof version === 'string' && (semver.parse(version, { loose: true })?.major ?? 0) >= 12
}

interface FindPlatformBinaryOptions {
  installDir: string
  wrapperDir: string
  wrapperPkgName: string
  executable: string
}

function findPlatformBinary (opts: FindPlatformBinaryOptions): string | undefined {
  const libcFamily = familySync()
  const candidateDirNames = [
    exePlatformPkgDirName(process.platform, process.arch, libcFamily),
    exePlatformPkgDirNameNext(process.platform, process.arch, libcFamily),
  ]
  for (const scopeDir of platformScopeDirs(opts)) {
    for (const dirName of candidateDirNames) {
      const candidate = path.join(scopeDir, dirName, opts.executable)
      if (fs.existsSync(candidate)) return candidate
    }
  }
  return undefined
}

function platformScopeDirs (opts: Pick<FindPlatformBinaryOptions, 'installDir' | 'wrapperDir' | 'wrapperPkgName'>): Set<string> {
  const wrapperRealDir = fs.realpathSync(opts.wrapperDir)
  const adjacentScopeDir = opts.wrapperPkgName.startsWith('@')
    ? path.dirname(wrapperRealDir)
    : path.join(path.dirname(wrapperRealDir), '@pnpm')
  // GVS dependencies link to sibling slots through the install root. The real
  // adjacent scope remains necessary for legacy virtual-store layouts.
  return new Set([
    adjacentScopeDir,
    path.join(opts.installDir, 'node_modules', '@pnpm'),
  ])
}

function linkWindowsAliases (src: string, wrapperDir: string): void {
  // Aliases (pn / pnpx / pnx) need to be .exe hardlinks of the native binary,
  // not the .cmd wrappers we ship in the tarball. cmd-shim's Bash shim for
  // a .cmd target wraps it in `exec cmd /C ...`, and MSYS2 / Git Bash
  // mangles `/C` into a Windows path — cmd.exe then falls into interactive
  // mode and prints its banner instead of running the alias. .exe sources
  // sidestep cmd-shim's wrapper. The native binary detects which name it was
  // launched as via process.execPath and prepends `dlx` for pnpx / pnx.
  // See https://github.com/pnpm/pnpm/issues/11486.
  for (const alias of ['pn', 'pnpx', 'pnx']) {
    forceLink(src, path.join(wrapperDir, `${alias}.exe`))
  }

  const wrapperPkgJsonPath = path.join(wrapperDir, 'package.json')
  const wrapperPkg = JSON.parse(fs.readFileSync(wrapperPkgJsonPath, 'utf8'))
  wrapperPkg.bin.pnpm = 'pnpm.exe'
  wrapperPkg.bin.pn = 'pn.exe'
  wrapperPkg.bin.pnpx = 'pnpx.exe'
  wrapperPkg.bin.pnx = 'pnx.exe'
  writePkgJsonWithoutMutatingStore(wrapperPkgJsonPath, wrapperPkg)
}

// Temp file + rename, not in-place: package.json is hard-linked from the
// content-addressable store, so writing in place would mutate the shared blob.
function writePkgJsonWithoutMutatingStore (pkgJsonPath: string, pkgJson: unknown): void {
  const tempPkgJsonPath = `${pkgJsonPath}.pnpm-tmp`
  try {
    fs.writeFileSync(tempPkgJsonPath, JSON.stringify(pkgJson, null, 2))
    fs.renameSync(tempPkgJsonPath, pkgJsonPath)
  } catch (err: unknown) {
    try {
      fs.rmSync(tempPkgJsonPath, { force: true })
    } catch {}
    throw err
  }
}

function forceLink (src: string, dest: string): void {
  try {
    fs.unlinkSync(dest)
  } catch (err: unknown) {
    if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') {
      throw err
    }
  }
  fs.linkSync(src, dest)
  fs.chmodSync(dest, 0o755)
}
