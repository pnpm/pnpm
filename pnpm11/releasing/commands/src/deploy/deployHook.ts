import { type BaseManifest, DEPENDENCIES_FIELDS } from '@pnpm/types'

export function deployHook<Pkg extends BaseManifest> (pkg: Pkg, opts?: { convertLinksToFileProtocol?: boolean }): Pkg {
  pkg.dependenciesMeta = pkg.dependenciesMeta ?? {}
  for (const depField of DEPENDENCIES_FIELDS) {
    for (const [depName, depVersion] of Object.entries(pkg[depField] ?? {})) {
      if ((depVersion as string).startsWith('workspace:')) {
        pkg.dependenciesMeta[depName] = {
          injected: true,
        }
      } else if (opts?.convertLinksToFileProtocol && (depVersion as string).startsWith('link:')) {
        pkg[depField]![depName] = `file:${(depVersion as string).slice(5)}`
      }
    }
  }
  return pkg
}
