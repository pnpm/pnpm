import { type ProjectManifest, DEPENDENCIES_FIELDS } from '@pnpm/types'

export function deployHook (pkg: ProjectManifest): ProjectManifest {
  pkg.dependenciesMeta = pkg.dependenciesMeta ?? {}
  for (const depField of DEPENDENCIES_FIELDS) {
    for (const [depName, depVersion] of Object.entries(pkg[depField] ?? {})) {
      if ((depVersion as string).startsWith('workspace:')) {
        pkg.dependenciesMeta[depName] = {
          injected: true,
        }
      }
    }
  }
  return pkg
}
