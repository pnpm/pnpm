import {
  RecursiveSummary,
  throwOnCommandFail,
} from '@pnpm/cli-utils'
import { Config } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import logger from '@pnpm/logger'
import { filterDependenciesByType } from '@pnpm/manifest-utils'
import matcher from '@pnpm/matcher'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import { requireHooks } from '@pnpm/pnpmfile'
import sortPackages from '@pnpm/sort-packages'
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
} from 'supi'
import getPinnedVersion from './getPinnedVersion'
import getSaveType from './getSaveType'
import updateToLatestSpecsFromManifest, { createLatestSpecs } from './updateToLatestSpecsFromManifest'
import { createWorkspaceSpecs, updateToWorkspacePackagesFromManifest } from './updateWorkspaceDependencies'
import path = require('path')
import camelcaseKeys = require('camelcase-keys')
import isSubdir = require('is-subdir')
import mem = require('mem')
import fs = require('mz/fs')
import pFilter = require('p-filter')
import pLimit = require('p-limit')
import readIniFile = require('read-ini-file')

type RecursiveOptions = CreateStoreControllerOptions & Pick<Config,
| 'bail'
| 'depth'
| 'globalPnpmfile'
| 'hoistPattern'
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
| 'sharedWorkspaceLockfile'
| 'tag'
> & {
  include?: IncludedDependencies
  includeDirect?: IncludedDependencies
  latest?: boolean
  pending?: boolean
  workspace?: boolean
} & Partial<Pick<Config, 'sort' | 'workspaceConcurrency'>>

export default async function recursive (
  allProjects: Project[],
  params: string[],
  opts: RecursiveOptions & {
    allowNew?: boolean
    forceHoistPattern?: boolean
    forcePublicHoistPattern?: boolean
    ignoredPackages?: Set<string>
    update?: boolean
    useBetaCli?: boolean
    selectedProjectsGraph: ProjectsGraph
  } & Required<Pick<Config, 'workspaceDir'>>,
  cmdFullName: 'install' | 'add' | 'remove' | 'unlink' | 'update'
): Promise<boolean | string> {
  if (allProjects.length === 0) {
    // It might make sense to throw an exception in this case
    return false
  }

  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)

  if (pkgs.length === 0) {
    return false
  }
  const manifestsByPath: { [dir: string]: Omit<Project, 'dir'> } = {}
  for (const { dir, manifest, writeProjectManifest } of pkgs) {
    manifestsByPath[dir] = { manifest, writeProjectManifest }
  }

  const throwOnFail = throwOnCommandFail.bind(null, `pnpm recursive ${cmdFullName}`)

  const chunks = opts.sort !== false
    ? sortPackages(opts.selectedProjectsGraph)
    : [Object.keys(opts.selectedProjectsGraph).sort()]

  const store = await createOrConnectStoreController(opts)

  const workspacePackages = cmdFullName !== 'unlink'
    ? arrayOfWorkspacePackagesToMap(allProjects)
    : {}
  const targetDependenciesField = getSaveType(opts)
  const installOpts = Object.assign(opts, {
    linkWorkspacePackagesDepth: opts.linkWorkspacePackages === 'deep' ? Infinity : opts.linkWorkspacePackages ? 0 : -1,
    ownLifecycleHooksStdio: 'pipe',
    peer: opts.savePeer,
    pruneLockfileImporters: (!opts.ignoredPackages || opts.ignoredPackages.size === 0) &&
      pkgs.length === allProjects.length,
    storeController: store.ctrl,
    storeDir: store.dir,
    targetDependenciesField,
    workspacePackages,

    forceHoistPattern: typeof opts.rawLocalConfig['hoist-pattern'] !== 'undefined' || typeof opts.rawLocalConfig['hoist'] !== 'undefined',
    forceShamefullyHoist: typeof opts.rawLocalConfig['shamefully-hoist'] !== 'undefined',
  }) as InstallOptions

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const memReadLocalConfig = mem(readLocalConfig)

  async function getImporters () {
    const importers = [] as Array<{ buildIndex: number, manifest: ProjectManifest, rootDir: string }>
    await Promise.all(chunks.map((prefixes: string[], buildIndex) => {
      if (opts.ignoredPackages) {
        prefixes = prefixes.filter((prefix) => !opts.ignoredPackages!.has(prefix))
      }
      return Promise.all(
        prefixes.map(async (prefix) => {
          importers.push({
            buildIndex,
            manifest: manifestsByPath[prefix].manifest,
            rootDir: prefix,
          })
        })
      )
    }))
    return importers
  }

  const updateToLatest = opts.update && opts.latest
  const includeDirect = opts.includeDirect ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }

  const updateMatch = cmdFullName === 'update' && params.length ? createMatcher(params) : null

  // For a workspace with shared lockfile
  if (opts.lockfileDir && ['add', 'install', 'remove', 'update'].includes(cmdFullName)) {
    let importers = await getImporters()
    const calculatedRepositoryRoot = calculateRepositoryRoot(opts.workspaceDir, importers.map(x => x.rootDir))
    const isFromWorkspace = isSubdir.bind(null, calculatedRepositoryRoot)
    importers = await pFilter(importers, async ({ rootDir }: { rootDir: string }) => isFromWorkspace(await fs.realpath(rootDir)))
    if (importers.length === 0) return true
    const hooks = opts.ignorePnpmfile ? {} : requireHooks(opts.lockfileDir, opts)
    const mutation = cmdFullName === 'remove' ? 'uninstallSome' : (params.length === 0 && !updateToLatest ? 'install' : 'installSome')
    const writeProjectManifests = [] as Array<(manifest: ProjectManifest) => Promise<void>>
    const mutatedImporters = [] as MutatedProject[]
    await Promise.all(importers.map(async ({ buildIndex, rootDir }) => {
      const localConfig = await memReadLocalConfig(rootDir)
      const modulesDir = localConfig.modulesDir ?? opts.modulesDir
      const { manifest, writeProjectManifest } = manifestsByPath[rootDir]
      let currentInput = [...params]
      if (updateMatch) {
        currentInput = matchDependencies(updateMatch, manifest, includeDirect)
        if (!currentInput.length && (typeof opts.depth === 'undefined' || opts.depth <= 0)) {
          installOpts.pruneLockfileImporters = false
          return
        }
      }
      if (updateToLatest) {
        if (!params || !params.length) {
          currentInput = updateToLatestSpecsFromManifest(manifest, includeDirect)
        } else {
          currentInput = createLatestSpecs(currentInput, manifest)
          if (!currentInput.length) {
            installOpts.pruneLockfileImporters = false
            return
          }
        }
      }
      if (opts.workspace) {
        if (!currentInput || !currentInput.length) {
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
          manifest,
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
          manifest,
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
          buildIndex,
          manifest,
          modulesDir,
          mutation,
          rootDir,
        } as MutatedProject)
      }
    }))
    if (!mutatedImporters.length && cmdFullName === 'update') {
      throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
        'None of the specified packages were found in the dependencies of any of the projects.')
    }
    const mutatedPkgs = await mutateModules(mutatedImporters, {
      ...installOpts,
      hooks,
      storeController: store.ctrl,
    })
    if (opts.save !== false) {
      await Promise.all(
        mutatedPkgs
          .map(({ manifest }, index) => writeProjectManifests[index](manifest))
      )
    }
    return true
  }

  const pkgPaths = chunks.length === 0
    ? chunks[0]
    : Object.keys(opts.selectedProjectsGraph).sort()

  const limitInstallation = pLimit(opts.workspaceConcurrency ?? 4)
  await Promise.all(pkgPaths.map((rootDir: string) =>
    limitInstallation(async () => {
      const hooks = opts.ignorePnpmfile ? {} : requireHooks(rootDir, opts)
      try {
        if (opts.ignoredPackages?.has(rootDir)) {
          return
        }

        const { manifest, writeProjectManifest } = manifestsByPath[rootDir]
        let currentInput = [...params]
        if (updateMatch) {
          currentInput = matchDependencies(updateMatch, manifest, includeDirect)
          if (!currentInput.length) return
        }
        if (updateToLatest) {
          if (!params || !params.length) {
            currentInput = updateToLatestSpecsFromManifest(manifest, includeDirect)
          } else {
            currentInput = createLatestSpecs(currentInput, manifest)
            if (!currentInput.length) return
          }
        }
        if (opts.workspace) {
          if (!currentInput || !currentInput.length) {
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
                manifest,
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
            : (manifest: PackageManifest, opts: any) => addDependenciesToPackage(manifest, currentInput, opts) // eslint-disable-line @typescript-eslint/no-explicit-any
          break
        }

        const localConfig = await memReadLocalConfig(rootDir)
        const newManifest = await action(
          manifest,
          {
            ...installOpts,
            ...localConfig,
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
      } catch (err) {
        logger.info(err)

        if (!opts.bail) {
          result.fails.push({
            error: err,
            message: err.message,
            prefix: rootDir,
          })
          return
        }

        err['prefix'] = rootDir // eslint-disable-line @typescript-eslint/dot-notation
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

  if (!result.passes && cmdFullName === 'update') {
    throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
      'None of the specified packages were found in the dependencies of any of the projects.')
  }

  return true
}

function unlink (manifest: ProjectManifest, opts: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
  return mutateModules(
    [
      {
        manifest,
        mutation: 'unlink',
        rootDir: opts.dir,
      },
    ],
    opts
  )
}

function unlinkPkgs (dependencyNames: string[], manifest: ProjectManifest, opts: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
  return mutateModules(
    [
      {
        dependencyNames,
        manifest,
        mutation: 'unlinkSome',
        rootDir: opts.dir,
      },
    ],
    opts
  )
}

async function readLocalConfig (prefix: string) {
  try {
    const ini = await readIniFile(path.join(prefix, '.npmrc')) as Record<string, string>
    const config = camelcaseKeys(ini) as (Record<string, string> & { hoist?: boolean })
    if (config.shamefullyFlatten) {
      config.hoistPattern = '*'
      // TODO: print a warning
    }
    if (config.hoist === false) {
      config.hoistPattern = ''
    }
    return config
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
    return {}
  }
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
    if (relativePartMatch) {
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

export function createMatcher (params: string[]) {
  const matchers = params.map((param) => {
    const atIndex = param.indexOf('@', 1)
    let pattern!: string
    let spec!: string
    if (atIndex === -1) {
      pattern = param
      spec = ''
    } else {
      pattern = param.substr(0, atIndex)
      spec = param.substr(atIndex + 1)
    }
    return {
      match: matcher(pattern),
      spec,
    }
  })
  return (depName: string) => {
    for (const { spec, match } of matchers) {
      if (match(depName)) return spec
    }
    return null
  }
}
