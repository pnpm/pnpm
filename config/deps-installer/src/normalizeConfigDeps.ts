import getNpmTarballUrl from 'get-npm-tarball-url'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import type { ConfigDependencies, Registries } from '@pnpm/types'
import type { NormalizedConfigDep } from './parseIntegrity.js'
import { parseIntegrity } from './parseIntegrity.js'

interface NormalizeConfigDepsOpts {
  registries: Registries
}

type NormalizedConfigDeps = Record<string, NormalizedConfigDep>

export function normalizeConfigDeps (configDependencies: ConfigDependencies, opts: NormalizeConfigDepsOpts): NormalizedConfigDeps {
  const deps: NormalizedConfigDeps = {}
  for (const [pkgName, pkgSpec] of Object.entries(configDependencies)) {
    const registry = pickRegistryForPackage(opts.registries, pkgName)

    if (typeof pkgSpec === 'object') {
      const { version, integrity } = parseIntegrity(pkgName, pkgSpec.integrity)
      deps[pkgName] = {
        version,
        resolution: {
          integrity,
          tarball: pkgSpec.tarball ? pkgSpec.tarball : getNpmTarballUrl(pkgName, version, { registry }),
        },
      }
      continue
    }

    if (typeof pkgSpec === 'string') {
      const { version, integrity } = parseIntegrity(pkgName, pkgSpec)
      deps[pkgName] = {
        version,
        resolution: {
          integrity,
          tarball: getNpmTarballUrl(pkgName, version, { registry }),
        },
      }
    }
  }

  return deps
}
