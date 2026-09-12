import path from 'node:path'

import type { HeadlessOptions } from '@pnpm/installing.deps-restorer'
import { readProjectsContext } from '@pnpm/installing.read-projects-context'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { getStorePath } from '@pnpm/store.path'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'
import type { DepPath, ProjectRootDir } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

/**
 * The options a test may override, on top of what this helper fills in.
 *
 * Typed rather than `any` so a key the headless install does not read is a
 * compile error here: an options object is the whole input to the subject
 * under test, and a misspelled or renamed key in one silently exercises the
 * default instead of the case the test is named for.
 */
export type TestHeadlessOptions = Partial<HeadlessOptions> & {
  /** Project directories, expanded into `HeadlessOptions.projects`. */
  projects?: string[]
  /**
   * Forwarded to the package store, which is what reads it — the headless
   * install itself has no such option.
   */
  verifyStoreIntegrity?: boolean
}

export async function testDefaults (
  opts?: TestHeadlessOptions,
  resolveOpts?: Record<string, unknown>,
  fetchOpts?: Record<string, unknown>,
  storeOpts?: Record<string, unknown>
): Promise<HeadlessOptions> {
  const tmp = temporaryDirectory()
  let storeDir = opts?.storeDir ?? path.join(tmp, 'store')
  const lockfileDir = opts?.lockfileDir ?? process.cwd()
  const { include, pendingBuilds, projects } = await readProjectsContext(
    opts?.projects
      ? opts.projects.map((rootDir) => ({ rootDir: rootDir as ProjectRootDir }))
      : [
        {
          rootDir: lockfileDir as ProjectRootDir,
        },
      ],
    { lockfileDir }
  )
  storeDir = await getStorePath({
    pkgRoot: lockfileDir,
    storePath: storeDir,
    pnpmHomeDir: '',
  })
  const { storeController } = createTempStore(
    {
      storeDir,
      clientOptions: {
        ...resolveOpts,
        ...fetchOpts,
      },
      storeOptions: {
        // The package store reads this, not the headless install.
        ...(opts?.verifyStoreIntegrity != null
          ? { verifyStoreIntegrity: opts.verifyStoreIntegrity }
          : {}),
        ...storeOpts,
      },
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
    selectedProjectDirs: opts?.selectedProjectDirs ?? projects.map((project) => project.rootDir),
    allProjects: Object.fromEntries(
      await Promise.all(projects.map(async (project) => [project.rootDir, { ...project, manifest: await safeReadPackageJsonFromDir(project.rootDir) }]))
    ),
    registriesByScope: {
      default: registry,
    },
    skipped: new Set<DepPath>(),
    storeController,
    storeDir,
    configByUri: {},
    globalVirtualStoreDir: path.join(storeDir, 'links'),
    ignoreScripts: false,
    pruneStore: false,
    sideEffectsCacheRead: false,
    sideEffectsCacheWrite: false,
    userAgent: 'pnpm/0.0.0 npm/? node/0.0.0 test test',
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    unsafePerm: true,
    ...opts,
  }
}
