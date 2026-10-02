import path from 'node:path'

import { LOCKFILE_VERSION } from '@pnpm/constants'
import {
  getWantedDependencies,
  resolveDependencies,
} from '@pnpm/installing.deps-resolver'
import type {
  LockfileObject,
  ProjectSnapshot,
} from '@pnpm/lockfile.types'
import type { StoreController } from '@pnpm/store.controller-types'
import type {
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
  RegistriesByScope,
} from '@pnpm/types'

export interface ResolveManifestDependenciesOpts {
  dir: string
  registriesByScope: RegistriesByScope
  storeController: StoreController
  storeDir: string
}

/**
 * Resolves the dependencies of a manifest and returns the resulting lockfile
 * without writing anything to disk.
 */
export async function resolveManifestDependencies (
  manifest: ProjectManifest,
  opts: ResolveManifestDependenciesOpts
): Promise<LockfileObject> {
  const dir = opts.dir as ProjectRootDir
  const emptyLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: { specifiers: {} } as ProjectSnapshot,
    },
  }
  const wantedDependencies = getWantedDependencies(manifest)
    .map((dep) => ({ ...dep, updateSpec: true }))

  const { newLockfile, waitTillAllFetchingsFinish } = await resolveDependencies(
    [
      {
        id: '.' as ProjectId,
        manifest,
        modulesDir: path.join(opts.dir, 'node_modules'),
        rootDir: dir,
        wantedDependencies,
        binsDir: path.join(opts.dir, 'node_modules', '.bin'),
        updatePackageManifest: false,
      },
    ],
    createResolveDependenciesOptions(opts, emptyLockfile)
  )
  await waitTillAllFetchingsFinish()
  return newLockfile
}

function createResolveDependenciesOptions (
  opts: ResolveManifestDependenciesOpts,
  emptyLockfile: LockfileObject
): Parameters<typeof resolveDependencies>[1] {
  return {
    allowedDeprecatedVersions: {},
    allowUnusedPatches: true,
    currentLockfile: emptyLockfile,
    defaultUpdateDepth: 0,
    dryRun: true,
    engineStrict: false,
    force: false,
    forceFullResolution: true,
    hooks: {},
    lockfileDir: opts.dir,
    nodeVersion: process.version,
    pnpmVersion: '',
    preferWorkspacePackages: false,
    preserveWorkspaceProtocol: false,
    registriesByScope: opts.registriesByScope,
    saveWorkspaceProtocol: false,
    storeController: opts.storeController,
    tag: 'latest',
    virtualStoreDir: path.join(opts.dir, 'node_modules', '.pnpm'),
    globalVirtualStoreDir: path.join(opts.storeDir, 'links'),
    virtualStoreDirMaxLength: 120,
    wantedLockfile: emptyLockfile,
    workspacePackages: new Map(),
    peersSuffixMaxLength: 1000,
    allProjectIds: ['.'],
  }
}
