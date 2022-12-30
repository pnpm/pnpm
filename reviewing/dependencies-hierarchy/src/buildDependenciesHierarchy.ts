import path from 'path'
import {
  getLockfileImporterId,
  Lockfile,
  ProjectSnapshot,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { normalizeRegistries } from '@pnpm/normalize-registries'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { DependenciesField, DEPENDENCIES_FIELDS, Registries } from '@pnpm/types'
import { refToRelative } from '@pnpm/dependency-path'
import normalizePath from 'normalize-path'
import realpathMissing from 'realpath-missing'
import resolveLinkTarget from 'resolve-link-target'
import { PackageNode } from './PackageNode'
import { SearchFunction } from './types'
import { getTree } from './getTree'
import { getPkgInfo } from './getPkgInfo'

export interface DependenciesHierarchy {
  dependencies?: PackageNode[]
  devDependencies?: PackageNode[]
  optionalDependencies?: PackageNode[]
  unsavedDependencies?: PackageNode[]
}

export async function buildDependenciesHierarchy (
  projectPaths: string[],
  maybeOpts: {
    depth: number
    include?: { [dependenciesField in DependenciesField]: boolean }
    registries?: Registries
    search?: SearchFunction
    lockfileDir: string
  }
): Promise<{ [projectDir: string]: DependenciesHierarchy }> {
  if (!maybeOpts?.lockfileDir) {
    throw new TypeError('opts.lockfileDir is required')
  }
  const modulesDir = await realpathMissing(path.join(maybeOpts.lockfileDir, 'node_modules'))
  const modules = await readModulesManifest(modulesDir)
  const registries = normalizeRegistries({
    ...maybeOpts?.registries,
    ...modules?.registries,
  })
  const currentLockfile = (modules?.virtualStoreDir && await readCurrentLockfile(modules.virtualStoreDir, { ignoreIncompatible: false })) ?? null

  const result = {} as { [projectDir: string]: DependenciesHierarchy }

  if (!currentLockfile) {
    for (const projectPath of projectPaths) {
      result[projectPath] = {}
    }
    return result
  }

  const opts = {
    depth: maybeOpts.depth || 0,
    include: maybeOpts.include ?? {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: maybeOpts.lockfileDir,
    registries,
    search: maybeOpts.search,
    skipped: new Set(modules?.skipped ?? []),
  }
  ; (
    await Promise.all(projectPaths.map(async (projectPath) => {
      return [
        projectPath,
        await dependenciesHierarchyForPackage(projectPath, currentLockfile, opts),
      ] as [string, DependenciesHierarchy]
    }))
  ).forEach(([projectPath, dependenciesHierarchy]) => {
    result[projectPath] = dependenciesHierarchy
  })
  return result
}

async function dependenciesHierarchyForPackage (
  projectPath: string,
  currentLockfile: Lockfile,
  opts: {
    depth: number
    include: { [dependenciesField in DependenciesField]: boolean }
    registries: Registries
    search?: SearchFunction
    skipped: Set<string>
    lockfileDir: string
  }
) {
  const importerId = getLockfileImporterId(opts.lockfileDir, projectPath)

  if (!currentLockfile.importers[importerId]) return {}

  const modulesDir = path.join(projectPath, 'node_modules')

  const savedDeps = getAllDirectDependencies(currentLockfile.importers[importerId])
  const allDirectDeps = await readModulesDir(modulesDir) ?? []
  const unsavedDeps = allDirectDeps.filter((directDep) => !savedDeps[directDep])
  const wantedLockfile = await readWantedLockfile(opts.lockfileDir, { ignoreIncompatible: false }) ?? { packages: {} }

  const getChildrenTree = getTree.bind(null, {
    currentPackages: currentLockfile.packages ?? {},
    includeOptionalDependencies: opts.include.optionalDependencies,
    lockfileDir: opts.lockfileDir,
    maxDepth: opts.depth,
    modulesDir,
    registries: opts.registries,
    search: opts.search,
    skipped: opts.skipped,
    wantedPackages: wantedLockfile.packages ?? {},
  })
  const result: DependenciesHierarchy = {}
  for (const dependenciesField of DEPENDENCIES_FIELDS.sort().filter(dependenciedField => opts.include[dependenciedField])) {
    const topDeps = currentLockfile.importers[importerId][dependenciesField] ?? {}
    result[dependenciesField] = []
    Object.entries(topDeps).forEach(([alias, ref]) => {
      const { packageInfo, packageAbsolutePath } = getPkgInfo({
        alias,
        currentPackages: currentLockfile.packages ?? {},
        modulesDir,
        ref,
        registries: opts.registries,
        skipped: opts.skipped,
        wantedPackages: wantedLockfile.packages ?? {},
      })
      let newEntry: PackageNode | null = null
      const matchedSearched = opts.search?.(packageInfo)
      if (packageAbsolutePath === null) {
        if ((opts.search != null) && !matchedSearched) return
        newEntry = packageInfo
      } else {
        const relativeId = refToRelative(ref, alias)
        if (relativeId) {
          const dependencies = getChildrenTree([relativeId], relativeId)
          if (dependencies.length > 0) {
            newEntry = {
              ...packageInfo,
              dependencies,
            }
          } else if ((opts.search == null) || matchedSearched) {
            newEntry = packageInfo
          }
        }
      }
      if (newEntry != null) {
        if (matchedSearched) {
          newEntry.searched = true
        }
        result[dependenciesField]!.push(newEntry)
      }
    })
  }

  await Promise.all(
    unsavedDeps.map(async (unsavedDep) => {
      let pkgPath = path.join(modulesDir, unsavedDep)
      let version!: string
      try {
        pkgPath = await resolveLinkTarget(pkgPath)
        version = `link:${normalizePath(path.relative(projectPath, pkgPath))}`
      } catch (err: any) { // eslint-disable-line
        // if error happened. The package is not a link
        const pkg = await safeReadPackageJsonFromDir(pkgPath)
        version = pkg?.version ?? 'undefined'
      }
      const pkg = {
        alias: unsavedDep,
        isMissing: false,
        isPeer: false,
        isSkipped: false,
        name: unsavedDep,
        path: pkgPath,
        version,
      }
      const matchedSearched = opts.search?.(pkg)
      if ((opts.search != null) && !matchedSearched) return
      const newEntry: PackageNode = pkg
      if (matchedSearched) {
        newEntry.searched = true
      }
      result.unsavedDependencies = result.unsavedDependencies ?? []
      result.unsavedDependencies.push(newEntry)
    })
  )

  return result
}

function getAllDirectDependencies (projectSnapshot: ProjectSnapshot) {
  return {
    ...projectSnapshot.dependencies,
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.optionalDependencies,
  }
}
