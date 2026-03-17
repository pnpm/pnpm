import path from 'node:path'

import { LOCKFILE_VERSION } from '@pnpm/constants'
import type {
  LockfileObject,
  ProjectSnapshot,
} from '@pnpm/lockfile.types'
import {
  getWantedDependencies,
  resolveDependencies,
} from '@pnpm/resolve-dependencies'
import type { StoreController } from '@pnpm/store-controller-types'
import type {
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
  Registries,
} from '@pnpm/types'

export interface ResolveManifestDependenciesOpts {
  dir: string
  registries: Registries
  storeController: StoreController
  storeDir: string
}

/**
 * Resolves the dependencies of a manifest and returns the resulting lockfile
 * without writing anything to disk.
 *
 * This is a lightweight wrapper around resolveDependencies for cases where
 * you only need the lockfile output (e.g., resolving package manager integrities).
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
    {
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
      registries: opts.registries,
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
  )
  await waitTillAllFetchingsFinish()
  return newLockfile
}
