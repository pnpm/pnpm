import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import { HeadlessOptions } from '@pnpm/headless'
import createStore from '@pnpm/package-store'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import readProjectsContext from '@pnpm/read-projects-context'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import storePath from '@pnpm/store-path'
import path = require('path')
import tempy = require('tempy')

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

const retryOpts = {
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMaxtimeout: 60_000,
  fetchRetryMintimeout: 10_000,
}

export default async function testDefaults (
  opts?: any, // tslint:disable-line
  resolveOpts?: any, // tslint:disable-line
  fetchOpts?: any, // tslint:disable-line
  storeOpts?: any, // tslint:disable-line
): Promise<HeadlessOptions> {
  let storeDir = opts?.storeDir ?? tempy.directory()
  const lockfileDir = opts?.lockfileDir ?? process.cwd()
  const { include, pendingBuilds, projects, registries } = await readProjectsContext(
    [
      {
        rootDir: lockfileDir,
      },
    ],
    { lockfileDir }
  )
  storeDir = await storePath(lockfileDir, storeDir)
  const rawConfig = { registry }
  const storeController = await createStore(
    createResolver({
      metaCache: new Map(),
      rawConfig,
      storeDir,
      strictSsl: true,
      ...retryOpts,
      ...resolveOpts,
    }),
    createFetcher({
      alwaysAuth: true,
      rawConfig,
      registry,
      ...retryOpts,
      ...fetchOpts,
    }),
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
    include,
    lockfileDir,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    pendingBuilds,
    projects: opts.projects ? opts.projects : await Promise.all(
      projects.map(async (project) => ({ ...project, manifest: await readPackageJsonFromDir(project.rootDir) }))
    ),
    rawConfig: {},
    registries: registries || {
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
