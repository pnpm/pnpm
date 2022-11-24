import { promises as fs } from 'fs'
import path from 'path'
import {
  RecursiveSummary,
  throwOnCommandFail,
} from '@pnpm/cli-utils'
import { Config, readLocalConfig } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import { logger } from '@pnpm/logger'
import { filterDependenciesByType } from '@pnpm/manifest-utils'
import { createMatcherWithIndex } from '@pnpm/matcher'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import { requireHooks } from '@pnpm/pnpmfile'
import { sortPackages } from '@pnpm/sort-packages'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import {
  IncludedDependencies,
  PackageManifest,
  Project,
  ProjectManifest,
  ProjectsGraph,
} from '@pnpm/types'
import {
  addDependenciesToPackage,
  install,
  InstallOptions,
  MutatedProject,
  mutateModules,
  ProjectOptions,
} from '@pnpm/core'
import isSubdir from 'is-subdir'
import mem from 'mem'
import pFilter from 'p-filter'
import pLimit from 'p-limit'
import { getOptionsFromRootManifest } from './getOptionsFromRootManifest'
import { createWorkspaceSpecs, updateToWorkspacePackagesFromManifest } from './updateWorkspaceDependencies'
import { updateToLatestSpecsFromManifest, createLatestSpecs } from './updateToLatestSpecsFromManifest'
import { getSaveType } from './getSaveType'
import { getPinnedVersion } from './getPinnedVersion'
import { PreferredVersions } from '@pnpm/resolver-base'

type RecursiveOptions = CreateStoreControllerOptions & Pick<Config,
| 'bail'
| 'depth'
| 'globalPnpmfile'
| 'hoistPattern'
| 'hooks'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'linkWorkspacePackages'
| 'lockfileDir'
| 'lockfileOnly'
| 'modulesDir'
| 'pnpmfile'
| 'rawLocalConfig'
| 'registries'
| 'save'
| 'saveDev'
| 'saveExact'
| 'saveOptional'
| 'savePeer'
| 'savePrefix'
| 'saveProd'
| 'saveWorkspaceProtocol'
| 'lockfileIncludeTarballUrl'
| 'sharedWorkspaceLockfile'
| 'tag'
> & {
  include?: IncludedDependencies
  includeDirect?: IncludedDependencies
  latest?: boolean
  pending?: boolean
  workspace?: boolean
  allowNew?: boolean
  forceHoistPattern?: boolean
  forcePublicHoistPattern?: boolean
  ignoredPackages?: Set<string>
  update?: boolean
  useBetaCli?: boolean
  allProjectsGraph: ProjectsGraph
  selectedProjectsGraph: ProjectsGraph
  preferredVersions?: PreferredVersions
  pruneDirectDependencies?: boolean
} & Partial<
Pick<Config,
| 'sort'
| 'workspaceConcurrency'
>
> & Required<
Pick<Config, 'workspaceDir'>
>

export async function recursive (
  allProjects: Project[],
  params: string[],
  opts: RecursiveOptions,
  cmdFullName: 'install' | 'add' | 'remove' | 'unlink' | 'update' | 'import'
): Promise<boolean | string> {
  if (allProjects.length === 0) {
    // It might make sense to throw an exception in this case
    return false
  }

  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)

  if (pkgs.length === 0) {
    return false
  }
  const manifestsByPath = getManifestsByPath(allProjects)

  const throwOnFail = throwOnCommandFail.bind(null, `pnpm recursive ${cmdFullName}`)

  const store = await createOrConnectStoreController(opts)

  const workspacePackages = cmdFullName !== 'unlink'
    ? arrayOfWorkspacePackagesToMap(allProjects)
    : {}
  const targetDependenciesField = getSaveType(opts)
  const installOpts = Object.assign(opts, {
    ...getOptionsFromRootManifest(manifestsByPath[opts.lockfileDir ?? opts.dir]?.manifest ?? {}),
    allProjects: getAllProjects(manifestsByPath, opts.allProjectsGraph, opts.sort),
    linkWorkspacePackagesDepth: opts.linkWorkspacePackages === 'deep' ? Infinity : opts.linkWorkspacePackages ? 0 : -1,
    ownLifecycleHooksStdio: 'pipe',
    peer: opts.savePeer,
    pruneLockfileImporters: ((opts.ignoredPackages == null) || opts.ignoredPackages.size === 0) &&
      pkgs.length === allProjects.length,
    storeController: store.ctrl,
    storeDir: store.dir,
    targetDependenciesField,
    workspacePackages,

    forceHoistPattern: typeof opts.rawLocalConfig?.['hoist-pattern'] !== 'undefined' || typeof opts.rawLocalConfig?.['hoist'] !== 'undefined',
    forceShamefullyHoist: typeof opts.rawLocalConfig?.['shamefully-hoist'] !== 'undefined',
  }) as InstallOptions

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const memReadLocalConfig = mem(readLocalConfig)

  const updateToLatest = opts.update && opts.latest
  const includeDirect = opts.includeDirect ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }

  let updateMatch: UpdateDepsMatcher | null
  if (cmdFullName === 'update') {
    if (params.length === 0) {
      const ignoreDeps = manifestsByPath[opts.workspaceDir]?.manifest?.pnpm?.updateConfig?.ignoreDependencies
      if (ignoreDeps?.length) {
        params = makeIgnorePatterns(ignoreDeps)
      }
    }
    updateMatch = params.length ? createMatcher(params) : null
  } else {
    updateMatch = null
  }
  // For a workspace with shared lockfile
  if (opts.lockfileDir && ['add', 'install', 'remove', 'update', 'import'].includes(cmdFullName)) {
    let importers = getImporters(opts)
    const calculatedRepositoryRoot = await fs.realpath(calculateRepositoryRoot(opts.workspaceDir, importers.map(x => x.rootDir)))
    const isFromWorkspace = isSubdir.bind(null, calculatedRepositoryRoot)
    importers = await pFilter(importers, async ({ rootDir }: { rootDir: string }) => isFromWorkspace(await fs.realpath(rootDir)))
    if (importers.length === 0) return true
    let mutation!: string
    switch (cmdFullName) {
    case 'remove':
      mutation = 'uninstallSome'
      break
    case 'import':
      mutation = 'install'
      break
    default:
      mutation = (params.length === 0 && !updateToLatest ? 'install' : 'installSome')
      break
    }
    const writeProjectManifests = [] as Array<(manifest: ProjectManifest) => Promise<void>>
    const mutatedImporters = [] as MutatedProject[]
    await Promise.all(importers.map(async ({ rootDir }) => {
      const localConfig = await memReadLocalConfig(rootDir)
      const modulesDir = localConfig.modulesDir ?? opts.modulesDir
      const { manifest, writeProjectManifest } = manifestsByPath[rootDir]
      let currentInput = [...params]
      if (updateMatch != null) {
        currentInput = matchDependencies(updateMatch, manifest, includeDirect)
        if ((currentInput.length === 0) && (typeof opts.depth === 'undefined' || opts.depth <= 0)) {
          installOpts.pruneLockfileImporters = false
          return
        }
      }
      if (updateToLatest) {
        if (!params || (params.length === 0)) {
          currentInput = updateToLatestSpecsFromManifest(manifest, includeDirect)
        } else {
          currentInput = createLatestSpecs(currentInput, manifest)
          if (currentInput.length === 0) {
            installOpts.pruneLockfileImporters = false
            return
          }
        }
      }
      if (opts.workspace) {
        if (!currentInput || (currentInput.length === 0)) {
          currentInput = updateToWorkspacePackagesFromManifest(manifest, includeDirect, workspacePackages)
        } else {
          currentInput = createWorkspaceSpecs(currentInput, workspacePackages)
        }
      }
      writeProjectManifests.push(writeProjectManifest)
      switch (mutation) {
      case 'uninstallSome':
        mutatedImporters.push({
          dependencyNames: currentInput,
          modulesDir,
          mutation,
          rootDir,
          targetDependenciesField,
        } as MutatedProject)
        return
      case 'installSome':
        mutatedImporters.push({
          allowNew: cmdFullName === 'install' || cmdFullName === 'add',
          dependencySelectors: currentInput,
          modulesDir,
          mutation,
          peer: opts.savePeer,
          pinnedVersion: getPinnedVersion({
            saveExact: typeof localConfig.saveExact === 'boolean' ? localConfig.saveExact : opts.saveExact,
            savePrefix: typeof localConfig.savePrefix === 'string' ? localConfig.savePrefix : opts.savePrefix,
          }),
          rootDir,
          targetDependenciesField,
        } as MutatedProject)
        return
      case 'install':
        mutatedImporters.push({
          modulesDir,
          mutation,
          pruneDirectDependencies: opts.pruneDirectDependencies,
          rootDir,
        } as MutatedProject)
      }
    }))
    if ((mutatedImporters.length === 0) && cmdFullName === 'update' && opts.depth === 0) {
      throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
        'None of the specified packages were found in the dependencies of any of the projects.')
    }
    const mutatedPkgs = await mutateModules(mutatedImporters, {
      ...installOpts,
      storeController: store.ctrl,
    })
    if (opts.save !== false) {
      await Promise.all(
        mutatedPkgs
          .map(async ({ originalManifest, manifest }, index) => writeProjectManifests[index](originalManifest ?? manifest))
      )
    }
    return true
  }

  const pkgPaths = Object.keys(opts.selectedProjectsGraph).sort()

  const limitInstallation = pLimit(opts.workspaceConcurrency ?? 4)
  await Promise.all(pkgPaths.map(async (rootDir: string) =>
    limitInstallation(async () => {
      const hooks = opts.ignorePnpmfile
        ? {}
        : (() => {
          const pnpmfileHooks = requireHooks(rootDir, opts)
          return {
            ...opts.hooks,
            ...pnpmfileHooks,
            afterAllResolved: [...(pnpmfileHooks.afterAllResolved ?? []), ...(opts.hooks?.afterAllResolved ?? [])],
            readPackage: [...(pnpmfileHooks.readPackage ?? []), ...(opts.hooks?.readPackage ?? [])],
          }
        })()
      try {
        if (opts.ignoredPackages?.has(rootDir)) {
          return
        }

        const { manifest, writeProjectManifest } = manifestsByPath[rootDir]
        let currentInput = [...params]
        if (updateMatch != null) {
          currentInput = matchDependencies(updateMatch, manifest, includeDirect)
          if (currentInput.length === 0) return
        }
        if (updateToLatest) {
          if (!params || (params.length === 0)) {
            currentInput = updateToLatestSpecsFromManifest(manifest, includeDirect)
          } else {
            currentInput = createLatestSpecs(currentInput, manifest)
            if (currentInput.length === 0) return
          }
        }
        if (opts.workspace) {
          if (!currentInput || (currentInput.length === 0)) {
            currentInput = updateToWorkspacePackagesFromManifest(manifest, includeDirect, workspacePackages)
          } else {
            currentInput = createWorkspaceSpecs(currentInput, workspacePackages)
          }
        }

        let action!: any // eslint-disable-line @typescript-eslint/no-explicit-any
        switch (cmdFullName) {
        case 'unlink':
          action = (currentInput.length === 0 ? unlink : unlinkPkgs.bind(null, currentInput))
          break
        case 'remove':
          action = async (manifest: PackageManifest, opts: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
            const [{ manifest: newManifest }] = await mutateModules([
              {
                dependencyNames: currentInput,
                mutation: 'uninstallSome',
                rootDir,
              },
            ], opts)
            return newManifest
          }
          break
        default:
          action = currentInput.length === 0
            ? install
            : async (manifest: PackageManifest, opts: any) => addDependenciesToPackage(manifest, currentInput, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
          break
        }

        const localConfig = await memReadLocalConfig(rootDir)
        const newManifest = await action(
          manifest,
          {
            ...installOpts,
            ...localConfig,
            ...getOptionsFromRootManifest(manifest),
            bin: path.join(rootDir, 'node_modules', '.bin'),
            dir: rootDir,
            hooks,
            ignoreScripts: true,
            pinnedVersion: getPinnedVersion({
              saveExact: typeof localConfig.saveExact === 'boolean' ? localConfig.saveExact : opts.saveExact,
              savePrefix: typeof localConfig.savePrefix === 'string' ? localConfig.savePrefix : opts.savePrefix,
            }),
            rawConfig: {
              ...installOpts.rawConfig,
              ...localConfig,
            },
            storeController: store.ctrl,
          }
        )
        if (opts.save !== false) {
          await writeProjectManifest(newManifest)
        }
        result.passes++
      } catch (err: any) { // eslint-disable-line
        logger.info(err)

        if (!opts.bail) {
          result.fails.push({
            error: err,
            message: err.message,
            prefix: rootDir,
          })
          return
        }

        err['prefix'] = rootDir
        throw err
      }
    })
  ))

  if (
    !opts.lockfileOnly && !opts.ignoreScripts && (
      cmdFullName === 'add' ||
      cmdFullName === 'install' ||
      cmdFullName === 'update' ||
      cmdFullName === 'unlink'
    )
  ) {
    await rebuild.handler({
      ...opts,
      pending: opts.pending === true,
    }, [])
  }

  throwOnFail(result)

  if (!result.passes && cmdFullName === 'update' && opts.depth === 0) {
    throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
      'None of the specified packages were found in the dependencies of any of the projects.')
  }

  return true
}

async function unlink (manifest: ProjectManifest, opts: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
  return mutateModules(
    [
      {
        mutation: 'unlink',
        rootDir: opts.dir,
      },
    ],
    opts
  )
}

async function unlinkPkgs (dependencyNames: string[], manifest: ProjectManifest, opts: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
  return mutateModules(
    [
      {
        dependencyNames,
        mutation: 'unlinkSome',
        rootDir: opts.dir,
      },
    ],
    opts
  )
}

function calculateRepositoryRoot (
  workspaceDir: string,
  projectDirs: string[]
) {
  // assume repo root is workspace dir
  let relativeRepoRoot = '.'
  for (const rootDir of projectDirs) {
    const relativePartRegExp = new RegExp(`^(\\.\\.\\${path.sep})+`)
    const relativePartMatch = relativePartRegExp.exec(path.relative(workspaceDir, rootDir))
    if (relativePartMatch != null) {
      const relativePart = relativePartMatch[0]
      if (relativePart.length > relativeRepoRoot.length) {
        relativeRepoRoot = relativePart
      }
    }
  }
  return path.resolve(workspaceDir, relativeRepoRoot)
}

export function matchDependencies (
  match: (input: string) => string | null,
  manifest: ProjectManifest,
  include: IncludedDependencies
) {
  const deps = Object.keys(filterDependenciesByType(manifest, include))
  const matchedDeps = []
  for (const dep of deps) {
    const spec = match(dep)
    if (spec === null) continue
    matchedDeps.push(spec ? `${dep}@${spec}` : dep)
  }
  return matchedDeps
}

export type UpdateDepsMatcher = (input: string) => string | null

export function createMatcher (params: string[]): UpdateDepsMatcher {
  const patterns: string[] = []
  const specs: string[] = []
  for (const param of params) {
    const atIndex = param.indexOf('@', param[0] === '!' ? 2 : 1)
    if (atIndex === -1) {
      patterns.push(param)
      specs.push('')
    } else {
      patterns.push(param.slice(0, atIndex))
      specs.push(param.slice(atIndex + 1))
    }
  }
  const matcher = createMatcherWithIndex(patterns)
  return (depName: string) => {
    const index = matcher(depName)
    if (index === -1) return null
    return specs[index]
  }
}

export function makeIgnorePatterns (ignoredDependencies: string[]): string[] {
  return ignoredDependencies.map(depName => `!${depName}`)
}

function getAllProjects (manifestsByPath: ManifestsByPath, allProjectsGraph: ProjectsGraph, sort?: boolean): ProjectOptions[] {
  const chunks = sort !== false
    ? sortPackages(allProjectsGraph)
    : [Object.keys(allProjectsGraph).sort()]
  return chunks.map((prefixes: string[], buildIndex) => prefixes.map((rootDir) => ({
    buildIndex,
    manifest: manifestsByPath[rootDir].manifest,
    rootDir,
  }))).flat()
}

interface ManifestsByPath { [dir: string]: Omit<Project, 'dir'> }

function getManifestsByPath (projects: Project[]) {
  return projects.reduce((manifestsByPath, { dir, manifest, writeProjectManifest }) => {
    manifestsByPath[dir] = { manifest, writeProjectManifest }
    return manifestsByPath
  }, {})
}

function getImporters (opts: Pick<RecursiveOptions, 'selectedProjectsGraph' | 'ignoredPackages'>) {
  let rootDirs = Object.keys(opts.selectedProjectsGraph)
  if (opts.ignoredPackages != null) {
    rootDirs = rootDirs.filter((rootDir) => !opts.ignoredPackages!.has(rootDir))
  }
  return rootDirs.map((rootDir) => ({ rootDir }))
}
