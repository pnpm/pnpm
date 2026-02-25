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

export function getCurrentPackageName (): string {
  return detectIfCurrentPkgIsExecutable() ? '@pnpm/exe' : 'pnpm'
}
