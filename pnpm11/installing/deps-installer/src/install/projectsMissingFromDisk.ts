import path from 'node:path'

import { parse as parseDepPath, refToRelative } from '@pnpm/deps.path'
import type { PnpmContext } from '@pnpm/installing.context'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { LockfileObject, PackageSnapshots, ProjectSnapshot } from '@pnpm/lockfile.types'
import {
  DEPENDENCIES_FIELDS,
  type ProjectId,
  type ProjectManifest,
  type ProjectRootDir,
  type ProjectRootDirRealPath,
} from '@pnpm/types'
import { isSubdir } from 'is-subdir'
import { clone } from 'ramda'

import type { ImporterToUpdate } from './mutationTypes.js'

export type ProjectMissingFromDisk = PnpmContext['projects'][string] & {
  keptLockfileImporter: ProjectSnapshot
}

/**
 * Stand-ins for the projects inside the workspace whose importers the wanted
 * lockfile records but that are not on disk, such as projects a sparse
 * checkout leaves out. Each one declares the specifiers its importer records,
 * so the resolution sees its dependencies as it does when the project is on
 * disk. A dependency recorded as a local path is left out: its target can be
 * relative to a directory that is not on disk.
 */
export function listProjectsMissingFromDisk (
  ctx: Pick<PnpmContext, 'lockfileDir' | 'projects' | 'wantedLockfile'>
): ProjectMissingFromDisk[] {
  const projectIds = new Set(Object.values(ctx.projects).map(({ id }) => id))
  return (Object.entries(ctx.wantedLockfile.importers ?? {}) as Array<[ProjectId, ProjectSnapshot]>)
    .map(([id, importer]) => ({ id, importer, rootDir: path.join(ctx.lockfileDir, id) as ProjectRootDir }))
    .filter(({ id, rootDir }) => !projectIds.has(id) && isSubdir(ctx.lockfileDir, rootDir))
    .map(({ id, importer, rootDir }) => {
      const modulesDir = path.join(rootDir, 'node_modules')
      return {
        id,
        buildIndex: 0,
        binsDir: path.join(modulesDir, '.bin'),
        manifest: manifestOfImporter(importer),
        modulesDir,
        rootDir,
        rootDirRealPath: rootDir as string as ProjectRootDirRealPath,
        keptLockfileImporter: clone(importer),
      }
    })
}

function manifestOfImporter (importer: ProjectSnapshot): ProjectManifest {
  const manifest: ProjectManifest = {}
  for (const field of DEPENDENCIES_FIELDS) {
    const specifiers: Record<string, string> = {}
    for (const [alias, ref] of Object.entries(importer[field] ?? {})) {
      const specifier = importer.specifiers[alias]
      if (specifier != null && !isLocalDependency(alias, ref)) {
        specifiers[alias] = specifier
      }
    }
    if (Object.keys(specifiers).length > 0) {
      manifest[field] = specifiers
    }
  }
  return manifest
}

/**
 * Whether a dependency is recorded as a local path, including an aliased
 * `<name>@file:` reference. The recorded reference decides it: a specifier
 * such as `repo.tgz` can also be a dist-tag.
 */
function isLocalDependency (alias: string, ref: string): boolean {
  const depPath = refToRelative(ref, alias)
  if (depPath == null) return true
  const version = parseDepPath(depPath).nonSemverVersion
  return version != null && (version.startsWith('file:') || version.startsWith('link:'))
}

/**
 * The resolution rewrites the importer of every project it resolves. A
 * project missing from disk keeps the entry the lockfile recorded, with the
 * packages of `previousPackages` that entry reaches. The resolution prunes
 * the packages only the recorded entry reaches, such as `file:` dependencies,
 * which its stand-in leaves out.
 */
export function keepImportersOfProjectsMissingFromDisk (
  lockfile: LockfileObject,
  { projects, previousPackages }: { projects: readonly ImporterToUpdate[], previousPackages: PackageSnapshots | undefined }
): LockfileObject {
  let keepsAnImporter = false
  for (const { id, keptLockfileImporter } of projects) {
    if (keptLockfileImporter == null) continue
    lockfile.importers[id] = keptLockfileImporter
    keepsAnImporter = true
  }
  if (!keepsAnImporter) return lockfile
  return pruneSharedLockfile({
    ...lockfile,
    packages: { ...previousPackages, ...lockfile.packages },
  })
}
