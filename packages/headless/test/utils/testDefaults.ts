import path from 'path'
import { createClient } from '@pnpm/client'
import { HeadlessOptions } from '@pnpm/headless'
import { createPackageStore } from '@pnpm/package-store'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { readProjectsContext } from '@pnpm/read-projects-context'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { getStorePath } from '@pnpm/store-path'
import fromPairs from 'ramda/src/fromPairs'
import tempy from 'tempy'

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
  const { include, pendingBuilds, projects } = await readProjectsContext(
    opts.projects
      ? opts.projects.map((rootDir: string) => ({ rootDir }))
      : [
        {
          rootDir: lockfileDir,
        },
      ],
    { lockfileDir }
  )
  storeDir = await getStorePath({
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
  const storeController = await createPackageStore(
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
    selectedProjectDirs: opts.selectedProjectDirs ?? projects.map((project) => project.rootDir),
    allProjects: fromPairs(
      await Promise.all(projects.map(async (project) => [project.rootDir, { ...project, manifest: await safeReadPackageJsonFromDir(project.rootDir) }]))
    ),
    rawConfig: {},
    registries: {
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
