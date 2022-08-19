import path from 'path'
import createClient from '@pnpm/client'
import { HeadlessOptions } from '@pnpm/headless'
import createStore from '@pnpm/package-store'
import readProjectsContext from '@pnpm/read-projects-context'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import storePath from '@pnpm/store-path'
import tempy from 'tempy'
import { toAllProjects, toAllProjectsMap } from './projects'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

const retryOpts = {
  factor: 10,
  retries: 2,
  retryMaxtimeout: 60_000,
  retryMintimeout: 10_000,
}

export default async function testDefaults (
  opts?: any, // eslint-disable-line
  resolveOpts?: any, // eslint-disable-line
  fetchOpts?: any, // eslint-disable-line
  storeOpts?: any, // eslint-disable-line
): Promise<HeadlessOptions> {
  const tmp = tempy.directory()
  let storeDir = opts?.storeDir ?? path.join(tmp, 'store')
  const cacheDir = path.join(tmp, 'cache')
  const lockfileDir = opts?.lockfileDir ?? process.cwd()
  const { include, pendingBuilds, projects, registries } = await readProjectsContext(
    [
      {
        rootDir: lockfileDir,
      },
    ],
    { lockfileDir }
  )
  const allProjects = opts.projects ? [] : await toAllProjects(projects)
  const allProjectsMap = opts.allProjectsMap ? opts.allProjectsMap : toAllProjectsMap(allProjects)

  storeDir = await storePath({
    pkgRoot: lockfileDir,
    storePath: storeDir,
    pnpmHomeDir: '',
  })
  const authConfig = { registry }
  const { resolve, fetchers } = createClient({
    authConfig,
    retry: retryOpts,
    cacheDir,
    ...resolveOpts,
    ...fetchOpts,
  })
  const storeController = await createStore(
    resolve,
    fetchers,
    {
      storeDir,
      ...storeOpts,
    }
  )
  return {
    currentEngine: {
      nodeVersion: process.version,
      pnpmVersion: '2.0.0',
    },
    engineStrict: false,
    force: false,
    hoistedDependencies: {},
    hoistPattern: ['*'],
    include,
    lockfileDir,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    pendingBuilds,
    projects: opts.projects
      ? opts.projects
      : allProjects,
    allProjectsMap,
    rawConfig: {},
    registries: registries ?? {
      default: registry,
    },
    sideEffectsCache: true,
    skipped: new Set<string>(),
    storeController,
    storeDir,
    unsafePerm: true,
    verifyStoreIntegrity: true,
    ...opts,
  }
}
