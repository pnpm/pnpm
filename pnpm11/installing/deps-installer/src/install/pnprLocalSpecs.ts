import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { isHostFilesystemSpecifier, isLocalFilesystemSpecifier } from '@pnpm/resolving.local-resolver'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { PackageExtension, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import type { MutatedProject } from './mutationTypes.js'

export { isHostFilesystemSpecifier }

export interface FindLocalSpecForPnprOptions {
  lockfileDir: string
  projects: Array<{ rootDir: ProjectRootDir, manifest: ProjectManifest }>
  mutations?: MutatedProject[]
  opts: {
    catalogs?: Catalogs
    overrides?: Record<string, string>
    packageExtensions?: Record<string, PackageExtension>
    registriesByPrefix?: Record<string, string>
  }
}

/**
 * First dependency specifier in the install inputs that resolves against the
 * client's filesystem. Returns `undefined` when the pnpr server can resolve
 * every specifier remotely.
 */
export function findLocalSpecForPnpr (input: FindLocalSpecForPnprOptions): string | undefined {
  const isDeclaredPrefix = (prefix: string): boolean => Boolean(input.opts.registriesByPrefix?.[prefix])
  return findLocalProjectSpec(input.projects, input.lockfileDir, isDeclaredPrefix) ??
    findLocalMutationSpec(input.mutations, input.lockfileDir, isDeclaredPrefix) ??
    findLocalCatalogSpec(input.opts.catalogs, isDeclaredPrefix) ??
    findLocalExtensionSpec(input.opts.packageExtensions, isDeclaredPrefix) ??
    findLocalOverrideSpec(input.opts.overrides, isDeclaredPrefix)
}

function findLocalProjectSpec (
  projects: Array<{ rootDir: ProjectRootDir, manifest: ProjectManifest }>,
  lockfileDir: string,
  isDeclaredPrefix: (prefix: string) => boolean
): string | undefined {
  for (const project of projects) {
    const importerDir = toPosixImporterDir(lockfileDir, project.rootDir)
    const found = findLocalManifestSpec(project.manifest, importerDir, isDeclaredPrefix)
    if (found != null) return found
  }
  return undefined
}

function findLocalManifestSpec (
  manifest: ProjectManifest,
  importerDir: string,
  isDeclaredPrefix: (prefix: string) => boolean
): string | undefined {
  const maps = [
    manifest.dependencies,
    manifest.devDependencies,
    manifest.optionalDependencies,
    manifest.peerDependencies,
  ]
  for (const dependencies of maps) {
    const found = findInDepMap(dependencies, importerDir, isDeclaredPrefix)
    if (found != null) return found
  }
  return undefined
}

function findLocalMutationSpec (
  mutations: MutatedProject[] | undefined,
  lockfileDir: string,
  isDeclaredPrefix: (prefix: string) => boolean
): string | undefined {
  if (!mutations) return undefined
  for (const mutation of mutations) {
    if (mutation.mutation !== 'installSome') continue
    const importerDir = toPosixImporterDir(lockfileDir, mutation.rootDir)
    const found = findLocalSelector(mutation.dependencySelectors, importerDir, isDeclaredPrefix)
    if (found != null) return found
  }
  return undefined
}

function findLocalSelector (
  selectors: string[],
  importerDir: string,
  isDeclaredPrefix: (prefix: string) => boolean
): string | undefined {
  for (const selector of selectors) {
    const parsed = parseWantedDependency(selector)
    if (!parsed.alias) {
      if (isLocalFilesystemSpecifier(selector) || isHostFilesystemSpecifier(selector, importerDir, isDeclaredPrefix)) {
        return selector
      }
      continue
    }
    if (parsed.bareSpecifier != null && isHostFilesystemSpecifier(parsed.bareSpecifier, importerDir, isDeclaredPrefix)) {
      return parsed.bareSpecifier
    }
  }
  return undefined
}

function findLocalCatalogSpec (
  catalogs: Catalogs | undefined,
  isDeclaredPrefix: (prefix: string) => boolean
): string | undefined {
  if (!catalogs) return undefined
  for (const catalog of Object.values(catalogs)) {
    if (!catalog) continue
    for (const specifier of Object.values(catalog)) {
      if (specifier != null && isHostFilesystemSpecifier(specifier, '.', isDeclaredPrefix)) {
        return specifier
      }
    }
  }
  return undefined
}

function findLocalExtensionSpec (
  extensions: Record<string, PackageExtension> | undefined,
  isDeclaredPrefix: (prefix: string) => boolean
): string | undefined {
  if (!extensions) return undefined
  for (const extension of Object.values(extensions)) {
    const maps = [
      extension.dependencies,
      extension.optionalDependencies,
      extension.peerDependencies,
    ]
    for (const dependencies of maps) {
      const found = findInDepMap(dependencies, '.', isDeclaredPrefix)
      if (found != null) return found
    }
  }
  return undefined
}

function findLocalOverrideSpec (
  overrides: Record<string, string> | undefined,
  isDeclaredPrefix: (prefix: string) => boolean
): string | undefined {
  return findInDepMap(overrides, '.', isDeclaredPrefix)
}

function findInDepMap (
  dependencies: Record<string, string> | undefined,
  importerDir: string,
  isDeclaredPrefix: (prefix: string) => boolean
): string | undefined {
  if (!dependencies) return undefined
  for (const specifier of Object.values(dependencies)) {
    if (isHostFilesystemSpecifier(specifier, importerDir, isDeclaredPrefix)) {
      return specifier
    }
  }
  return undefined
}

function toPosixImporterDir (lockfileDir: string, rootDir: string): string {
  const relative = path.relative(lockfileDir, rootDir).split(path.sep).join('/')
  return relative || '.'
}
