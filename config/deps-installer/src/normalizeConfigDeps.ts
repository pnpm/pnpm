import getNpmTarballUrl from 'get-npm-tarball-url'
import { PnpmError } from '@pnpm/error'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { type ConfigDependencies, type Registries } from '@pnpm/types'

interface NormalizeConfigDepsOpts {
  registries: Registries
}

type NormalizedConfigDeps = Record<string, {
  version: string
  resolution: {
    integrity: string
    tarball: string
  }
}>

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

function parseIntegrity (pkgName: string, pkgSpec: string) {
  const sepIndex = pkgSpec.indexOf('+')
  if (sepIndex === -1) {
    throw new PnpmError('CONFIG_DEP_NO_INTEGRITY', `Your config dependency called "${pkgName}" doesn't have an integrity checksum`, {
      hint: `Integrity checksum should be inlined in the version specifier. For example:

pnpm-workspace.yaml:
configDependencies:
  my-config: "1.0.0+sha512-Xg0tn4HcfTijTwfDwYlvVCl43V6h4KyVVX2aEm4qdO/PC6L2YvzLHFdmxhoeSA3eslcE6+ZVXHgWwopXYLNq4Q=="
`,
    })
  }
  const version = pkgSpec.substring(0, sepIndex)
  const integrity = pkgSpec.substring(sepIndex + 1)
  return { version, integrity }
}
