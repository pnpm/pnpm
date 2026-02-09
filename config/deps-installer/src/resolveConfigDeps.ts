import getNpmTarballUrl from 'get-npm-tarball-url'
import { PnpmError } from '@pnpm/error'
import { writeSettings } from '@pnpm/config.config-writer'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/fetch'
import { createNpmResolver, type ResolverFactoryOptions } from '@pnpm/npm-resolver'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import type { ConfigDependencies } from '@pnpm/types'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { installConfigDeps, type InstallConfigDepsOpts } from './installConfigDeps.js'

export type ResolveConfigDepsOpts = CreateFetchFromRegistryOptions & ResolverFactoryOptions & InstallConfigDepsOpts & {
  configDependencies?: ConfigDependencies
  rootDir: string
  userConfig?: Record<string, string>
}

export async function resolveConfigDeps (configDeps: string[], opts: ResolveConfigDepsOpts): Promise<void> {
  const fetch = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.userConfig!, userSettings: opts.userConfig })
  const { resolveFromNpm } = createNpmResolver(fetch, getAuthHeader, opts)
  const configDependencies = opts.configDependencies ?? {}
  await Promise.all(configDeps.map(async (configDep) => {
    const wantedDep = parseWantedDependency(configDep)
    if (!wantedDep.alias) {
      throw new PnpmError('BAD_CONFIG_DEP', `Cannot install ${configDep} as configuration dependency`)
    }
    const resolution = await resolveFromNpm(wantedDep, {
      lockfileDir: opts.rootDir,
      preferredVersions: {},
      projectDir: opts.rootDir,
    })
    if (resolution?.resolution == null || !('integrity' in resolution.resolution)) {
      throw new PnpmError('BAD_CONFIG_DEP', `Cannot install ${configDep} as configuration dependency because it has no integrity`)
    }
    const pkgName = wantedDep.alias
    const version = resolution.manifest.version
    const { tarball, integrity } = resolution.resolution
    const registry = pickRegistryForPackage(opts.registries, pkgName)
    const defaultTarball = getNpmTarballUrl(pkgName, version, { registry })
    if (tarball !== defaultTarball && isValidHttpUrl(tarball)) {
      configDependencies[pkgName] = {
        tarball,
        integrity: `${version}+${integrity}`,
      }
    } else {
      configDependencies[pkgName] = `${version}+${integrity}`
    }
  }))
  await writeSettings({
    ...opts,
    rootProjectManifestDir: opts.rootDir,
    workspaceDir: opts.rootDir,
    updatedSettings: {
      configDependencies,
    },
  })
  await installConfigDeps(configDependencies, opts)
}

function isValidHttpUrl (url: string): boolean {
  try {
    const parsed = new URL(url)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}
