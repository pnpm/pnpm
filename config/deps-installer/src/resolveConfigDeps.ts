import { PnpmError } from '@pnpm/error'
import { writeSettings } from '@pnpm/config.config-writer'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/fetch'
import { createNpmResolver, type ResolverFactoryOptions } from '@pnpm/npm-resolver'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import { type ConfigDependencies } from '@pnpm/types'
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
  const pkgTarballs: Record<string, string> = {}
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
    if (resolution?.resolution == null || !('integrity' in resolution?.resolution)) {
      throw new PnpmError('BAD_CONFIG_DEP', `Cannot install ${configDep} as configuration dependency because it has no integrity`)
    }
    configDependencies[wantedDep.alias] = `${resolution?.manifest?.version}+${resolution.resolution.integrity}`
    if (isValidHttpUrl(resolution.resolution.tarball)) {
      pkgTarballs[wantedDep.alias] = resolution.resolution.tarball
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

  Object.entries(pkgTarballs).forEach(([pkg, tarball]) => {
    // get-npm-tarball-url cannot determine the tarball URL of a private npm package hosted on GitHub Packages registry
    // therefore, we need to store the tarball URL separately for installConfigDeps to fetch correctly
    configDependencies[pkg] += ` ${tarball}`
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
