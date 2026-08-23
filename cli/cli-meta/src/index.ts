import path from 'path'
import { type DependencyManifest } from '@pnpm/types'
import loadJsonFile from 'load-json-file'

const defaultManifest = {
  name: process.env.npm_package_name != null && process.env.npm_package_name !== ''
    ? process.env.npm_package_name
    : 'pnpm',
  version: process.env.npm_package_version != null && process.env.npm_package_version !== ''
    ? process.env.npm_package_version
    : '0.0.0',
}
let pkgJson
if (require.main == null) {
  pkgJson = defaultManifest
} else {
  try {
    pkgJson = {
      ...defaultManifest,
      ...loadJsonFile.sync<DependencyManifest>(
        path.join(path.dirname(require.main.filename), '../package.json')
      ),
    }
  } catch {
    pkgJson = defaultManifest
  }
}

export const packageManager = {
  name: pkgJson.name,
  // Never a prerelease version
  stableVersion: pkgJson.version.includes('-')
    ? pkgJson.version.slice(0, pkgJson.version.indexOf('-'))
    : pkgJson.version,
  // This may be a 3.0.0-beta.2
  version: pkgJson.version,
}

export interface Process {
  arch: NodeJS.Architecture
  platform: NodeJS.Platform
  pkg?: unknown
}

export function detectIfCurrentPkgIsExecutable (proc: Process = process): boolean {
  return 'pkg' in proc && proc.pkg != null
}

export function isExecutedByCorepack (env: NodeJS.ProcessEnv = process.env): boolean {
  return env.COREPACK_ROOT != null
}

/**
 * The command that installs pnpm with the standalone script, as documented at
 * https://pnpm.io/installation: the PowerShell form for `win32`, the
 * `curl`-into-`sh` form for every other platform. Reads the platform from
 * `proc`, defaulting to the host's; always returns a command that can be run
 * as printed.
 */
export function standaloneInstallCommand (proc: Process = process): string {
  return proc.platform === 'win32'
    ? 'Invoke-WebRequest https://get.pnpm.io/install.ps1 -UseBasicParsing | Invoke-Expression'
    : 'curl -fsSL https://get.pnpm.io/install.sh | sh -'
}

export function getCurrentPackageName (proc: Process = process): string {
  return detectIfCurrentPkgIsExecutable(proc) ? '@pnpm/exe' : 'pnpm'
}
