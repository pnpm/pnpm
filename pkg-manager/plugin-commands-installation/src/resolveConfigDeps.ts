import { PnpmError } from '@pnpm/error'
import { writeSettings } from '@pnpm/config.config-writer'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createNpmResolver } from '@pnpm/npm-resolver'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import { type AddCommandOptions } from './add'

export async function resolveConfigDeps (configDeps: string[], opts: AddCommandOptions) {
  const fetch = createFetchFromRegistry({})
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.userConfig!, userSettings: opts.userConfig })
  const { resolveFromNpm } = createNpmResolver(fetch, getAuthHeader, opts)
  const configDependencies = opts.configDependencies ?? {}
  await Promise.all(configDeps.map(async (configDep) => {
    const wantedDep = parseWantedDependency(configDep)
    if (!wantedDep.alias) {
      throw new PnpmError('BAD_CONFIG_DEP', `Cannot install ${configDep} as configuration dependency`)
    }
    const resolution = await resolveFromNpm(wantedDep, {
      lockfileDir: opts.lockfileDir,
      preferredVersions: {},
      projectDir: opts.dir,
    })
    if (resolution?.resolution == null || !('integrity' in resolution?.resolution)) {
      throw new PnpmError('BAD_CONFIG_DEP', `Cannot install ${configDep} as configuration dependency because it has no integrity`)
    }
    configDependencies[wantedDep.alias] = `${resolution?.manifest?.version}+${resolution.resolution.integrity}`
  }))
  await writeSettings({
    ...opts,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
    updatedSettings: {
      configDependencies,
    },
  })
}
