import packageIsInstallable from '@pnpm/package-is-installable'
import packageManager from './pnpmPkgJson'

export default function (
  pkgPath: string,
  pkg: {
    name: string,
    version: string,
    engines?: {
      node?: string,
      npm?: string,
    },
    cpu?: string[],
    os?: string[],
  },
  opts: {
    engineStrict?: boolean,
  },
) {
  const warn = packageIsInstallable(pkgPath, pkg, {
    engineStrict: false,
    optional: false,
    pnpmVersion: packageManager.version,
    prefix: pkgPath,
  })
  if (warn === true) return
  if (warn['wanted'].pnpm || opts.engineStrict) throw warn
}
