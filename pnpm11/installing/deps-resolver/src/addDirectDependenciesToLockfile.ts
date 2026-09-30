import type { ProjectSnapshot } from '@pnpm/lockfile.types'
import {
  getAllDependenciesFromManifest,
  getSpecFromPackageManifest,
} from '@pnpm/pkg-manifest.utils'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type ProjectManifest,
} from '@pnpm/types'

import { depPathToRef } from './depPathToRef.js'
import type { LinkedDependency, ResolvedDirectDependency } from './resolveDependencyTree.js'

type NewProjectSnapshot = ProjectSnapshot & Required<Pick<ProjectSnapshot, 'dependencies' | 'devDependencies' | 'optionalDependencies'>>

export interface AddDirectDependenciesToLockfileOptions {
  directDependencies: ResolvedDirectDependency[]
  excludeLinksFromLockfile?: boolean
  linkedPackages: Array<{ alias: string }>
  newManifest: ProjectManifest
  projectSnapshot: ProjectSnapshot
}

export function addDirectDependenciesToLockfile (opts: AddDirectDependenciesToLockfileOptions): ProjectSnapshot {
  const { newManifest } = opts
  const newProjectSnapshot: NewProjectSnapshot = {
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
    specifiers: {},
  }

  addPublishDirectory(newProjectSnapshot, newManifest)

  for (const linkedPkg of opts.linkedPackages) {
    newProjectSnapshot.specifiers[linkedPkg.alias] = getSpecFromPackageManifest(newManifest, linkedPkg.alias)
  }

  const directDependenciesByAlias = new Map<string, ResolvedDirectDependency>()
  for (const directDependency of opts.directDependencies) {
    directDependenciesByAlias.set(directDependency.alias, directDependency)
  }

  const allDeps = Array.from(new Set(Object.keys(getAllDependenciesFromManifest(newManifest))))

  for (const alias of allDeps) {
    const dep = directDependenciesByAlias.get(alias)
    const spec = dep && getSpecFromPackageManifest(newManifest, dep.alias)
    if (dep && shouldLockDirectDependency(dep, { excludeLinksFromLockfile: opts.excludeLinksFromLockfile, spec: spec! })) {
      addResolvedDirectDependency(newProjectSnapshot, { dep, spec: spec! })
    } else if (opts.projectSnapshot.specifiers[alias]) {
      copyLockedDirectDependency(newProjectSnapshot, { alias, projectSnapshot: opts.projectSnapshot })
    }
  }

  alignDependencyTypes(newManifest, newProjectSnapshot)

  return newProjectSnapshot
}

function addPublishDirectory (newProjectSnapshot: NewProjectSnapshot, manifest: ProjectManifest): void {
  if (!manifest.publishConfig?.directory) return
  newProjectSnapshot.publishDirectory = manifest.publishConfig.directory
  if (manifest.publishConfig.linkDirectory === false) {
    newProjectSnapshot.linkDirectory = false
  }
}

function shouldLockDirectDependency (
  dep: ResolvedDirectDependency,
  { excludeLinksFromLockfile, spec }: { excludeLinksFromLockfile?: boolean, spec: string }
): boolean {
  return !excludeLinksFromLockfile ||
    !(dep as LinkedDependency).isLinkedDependency ||
    spec.startsWith('workspace:')
}

function addResolvedDirectDependency (
  newProjectSnapshot: NewProjectSnapshot,
  { dep, spec }: { dep: ResolvedDirectDependency, spec: string }
): void {
  const ref = depPathToRef(dep.pkgId, {
    alias: dep.alias,
    realName: dep.name,
  })
  if (dep.dev) {
    newProjectSnapshot.devDependencies[dep.alias] = ref
  } else if (dep.optional) {
    newProjectSnapshot.optionalDependencies[dep.alias] = ref
  } else {
    newProjectSnapshot.dependencies[dep.alias] = ref
  }
  newProjectSnapshot.specifiers[dep.alias] = spec
}

function copyLockedDirectDependency (
  newProjectSnapshot: NewProjectSnapshot,
  { alias, projectSnapshot }: { alias: string, projectSnapshot: ProjectSnapshot }
): void {
  newProjectSnapshot.specifiers[alias] = projectSnapshot.specifiers[alias]
  if (projectSnapshot.dependencies?.[alias]) {
    newProjectSnapshot.dependencies[alias] = projectSnapshot.dependencies[alias]
  } else if (projectSnapshot.optionalDependencies?.[alias]) {
    newProjectSnapshot.optionalDependencies[alias] = projectSnapshot.optionalDependencies[alias]
  } else if (projectSnapshot.devDependencies?.[alias]) {
    newProjectSnapshot.devDependencies[alias] = projectSnapshot.devDependencies[alias]
  }
}

function alignDependencyTypes (manifest: ProjectManifest, projectSnapshot: ProjectSnapshot): void {
  const depTypesOfAliases = getAliasToDependencyTypeMap(manifest)

  // Aligning the dependency types in pnpm-lock.yaml
  for (const depType of DEPENDENCIES_FIELDS) {
    if (projectSnapshot[depType] == null) continue
    for (const [alias, ref] of Object.entries(projectSnapshot[depType] ?? {})) {
      if (depType === depTypesOfAliases[alias] || !depTypesOfAliases[alias]) continue
      projectSnapshot[depTypesOfAliases[alias]]![alias] = ref
      delete projectSnapshot[depType]![alias]
    }
  }
}

function getAliasToDependencyTypeMap (manifest: ProjectManifest): Record<string, DependenciesField> {
  // Null-prototype: keyed by dependency aliases, which may be `constructor` or `toString`.
  const depTypesOfAliases: Record<string, DependenciesField> = Object.create(null)
  for (const depType of DEPENDENCIES_FIELDS) {
    if (manifest[depType] == null) continue
    for (const alias of Object.keys(manifest[depType] ?? {})) {
      if (!depTypesOfAliases[alias]) {
        depTypesOfAliases[alias] = depType
      }
    }
  }
  return depTypesOfAliases
}
