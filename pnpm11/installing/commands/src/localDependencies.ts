import path from 'node:path'

import { type LockfileObject, type LockfileResolution, readCurrentLockfile } from '@pnpm/lockfile.fs'
import { resolveLocalTarballPath } from '@pnpm/lockfile.utils'
import type { LocalDependencies, ReadLocalDependencies } from '@pnpm/workspace.state'

/**
 * Reads, for the workspace state, the local tarballs a recorded lockfile
 * resolves and the directories it resolves other than `workspaceProjectDirs`.
 */
export function createLocalDependenciesReader (
  workspaceProjectDirs: string[],
  readWantedLockfile: (lockfileDir: string) => Promise<LockfileObject | null>
): ReadLocalDependencies {
  const projectDirs = new Set(workspaceProjectDirs.map((dir) => path.resolve(dir)))
  return async ({ lockfileDir, current }) => {
    const lockfile = current
      ? await readCurrentLockfile(path.join(lockfileDir, 'node_modules/.pnpm'), { ignoreIncompatible: true })
      : await readWantedLockfile(lockfileDir)
    return lockfile == null ? {} : localDependenciesOf(lockfile, lockfileDir, projectDirs)
  }
}

function localDependenciesOf (lockfile: LockfileObject, lockfileDir: string, projectDirs: Set<string>): LocalDependencies {
  const tarballs = new Set<string>()
  const directories = new Set<string>()
  for (const { resolution } of Object.values(lockfile.packages ?? {})) {
    const tarball = localTarballOf(resolution, lockfileDir)
    if (tarball != null) tarballs.add(tarball)
    const dir = localDirectoryOf(resolution, lockfileDir)
    if (dir != null && !projectDirs.has(dir)) directories.add(dir)
  }
  return {
    ...tarballs.size > 0 ? { tarballs: [...tarballs].sort() } : {},
    ...directories.size > 0 ? { directories: [...directories].sort() } : {},
  }
}

function localTarballOf (resolution: LockfileResolution, lockfileDir: string): string | undefined {
  return 'tarball' in resolution && typeof resolution.tarball === 'string'
    ? resolveLocalTarballPath(lockfileDir, resolution.tarball)
    : undefined
}

function localDirectoryOf (resolution: LockfileResolution, lockfileDir: string): string | undefined {
  return 'type' in resolution && resolution.type === 'directory'
    ? path.resolve(lockfileDir, resolution.directory)
    : undefined
}
