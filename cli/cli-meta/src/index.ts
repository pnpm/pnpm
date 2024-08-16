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

export function detectIfCurrentPkgIsExecutable (proc: NodeJS.Process = process): boolean {
  return 'pkg' in proc && proc.pkg != null
}

export function isExecutedByCorepack (env: NodeJS.ProcessEnv = process.env): boolean {
  return env.COREPACK_ROOT != null
}

export function getCurrentPackageName () {
  return detectIfCurrentPkgIsExecutable() ? getExePackageName() : 'pnpm'
}

function getExePackageName () {
  const platform = process.platform === 'win32'
    ? 'win'
    : process.platform === 'darwin'
      ? 'macos'
      : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch

  return `@pnpm/${platform}-${arch}`
}
