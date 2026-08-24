const defaultManifest = {
  name: process.env.npm_package_name != null && process.env.npm_package_name !== ''
    ? process.env.npm_package_name
    : 'pnpm',
  version: process.env.npm_package_version != null && process.env.npm_package_version !== ''
    ? process.env.npm_package_version
    : '0.0.0',
}
const pkgJson = defaultManifest

export const packageManager = {
  name: pkgJson.name,
  // Never a prerelease version
  stableVersion: pkgJson.version.includes('-')
    ? pkgJson.version.slice(0, pkgJson.version.indexOf('-'))
    : pkgJson.version,
  // This may be a 3.0.0-beta.2
  version: pkgJson.version,
}

export function detectIfCurrentPkgIsExecutable (_proc?: unknown): boolean {
  try {
    // require() is available here because esbuild injects a createRequire shim
    // via the banner in pnpm/bundle.ts. node:sea is not available as an ESM
    // import, so require() is the correct approach.
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    return require('node:sea').isSea()
  } catch {
    return false
  }
}

export function isExecutedByCorepack (env: NodeJS.ProcessEnv = process.env): boolean {
  return env.COREPACK_ROOT != null
}

/**
 * The command that installs pnpm with the standalone script, as documented at
 * https://pnpm.io/installation: the PowerShell form for `win32`, the
 * `curl`-into-`sh` form for every other `platform`. Defaults to the host's
 * platform; always returns a command that can be run as printed.
 */
export function standaloneInstallCommand (platform: NodeJS.Platform = process.platform): string {
  return platform === 'win32'
    ? 'Invoke-WebRequest https://get.pnpm.io/install.ps1 -UseBasicParsing | Invoke-Expression'
    : 'curl -fsSL https://get.pnpm.io/install.sh | sh -'
}

export function getCurrentPackageName (): string {
  return detectIfCurrentPkgIsExecutable() ? '@pnpm/exe' : 'pnpm'
}
