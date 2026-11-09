import path from 'node:path'

/**
 * The staging directory an install builds a global virtual store package in:
 * `.<name>-<token>` beside the package directory, so a scoped package stages
 * in its scope directory. `token` is unique to the install.
 */
export function getStagingDir (pkgDir: string, token: string): string {
  return path.join(path.dirname(pkgDir), `${stagingDirPrefix(path.basename(pkgDir))}${token}`)
}

/** Whether `entryName`, beside a package directory named `pkgDirName`, is one of its staging directories. */
export function isStagingDirName (pkgDirName: string, entryName: string): boolean {
  return entryName.startsWith(stagingDirPrefix(pkgDirName))
}

function stagingDirPrefix (pkgDirName: string): string {
  return `.${pkgDirName}-`
}
