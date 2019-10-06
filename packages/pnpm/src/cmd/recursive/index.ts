import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import { DependencyManifest, ImporterManifest, PackageManifest } from '@pnpm/types'
import camelcaseKeys = require('camelcase-keys')
import graphSequencer = require('graph-sequencer')
import isSubdir = require('is-subdir')
import mem = require('mem')
import fs = require('mz/fs')
import pFilter = require('p-filter')
import pLimit from 'p-limit'
import path = require('path')
import createPkgGraph, { PackageNode } from 'pkgs-graph'
import readIniFile = require('read-ini-file')
import {
  addDependenciesToPackage,
  install,
  InstallOptions,
  MutatedImporter,
  mutateModules,
  rebuild,
  rebuildPkgs,
} from 'supi'
import createStoreController from '../../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../../findWorkspacePackages'
import getCommandFullName from '../../getCommandFullName'
import getPinnedVersion from '../../getPinnedVersion'
import getSaveType from '../../getSaveType'
import { scopeLogger } from '../../loggers'
import parsePackageSelector, { PackageSelector } from '../../parsePackageSelectors'
import requireHooks from '../../requireHooks'
import { PnpmOptions } from '../../types'
import updateToLatestSpecsFromManifest, { createLatestSpecs } from '../../updateToLatestSpecsFromManifest'
import help from '../help'
import exec from './exec'
import { filterGraph } from './filter'
import list from './list'
import outdated from './outdated'
import RecursiveSummary, { throwOnCommandFail } from './recursiveSummary'
import run from './run'

const supportedRecursiveCommands = new Set([
  'add',
  'install',
  'uninstall',
  'update',
  'unlink',
  'list',
  'why',
  'outdated',
  'rebuild',
  'run',
  'test',
  'exec',
])

export default async (
  input: string[],
  opts: PnpmOptions,
) => {
  if (opts.workspaceConcurrency < 1) {
    throw new PnpmError('INVALID_WORKSPACE_CONCURRENCY', 'Workspace concurrency should be at least 1')
  }

  const cmd = input.shift()
  if (!cmd) {
    help(['recursive'])
    return
  }
  const cmdFullName = getCommandFullName(cmd)
  if (!supportedRecursiveCommands.has(cmdFullName)) {
    help(['recursive'])
    throw new PnpmError('INVALID_RECURSIVE_COMMAND',
      `"recursive ${cmdFullName}" is not a pnpm command. See "pnpm help recursive".`)
  }

  const workspacePrefix = opts.workspacePrefix || process.cwd()
  const allWorkspacePkgs = await findWorkspacePackages(workspacePrefix, opts)

  if (!allWorkspacePkgs.length) {
    logger.info({ message: `No packages found in "${workspacePrefix}"`, prefix: workspacePrefix })
    return
  }

  if (opts.filter) {
    // TODO: maybe @pnpm/config should return this in a parsed form already?
    // We don't use opts.prefix in this case because opts.prefix searches for a package.json in parent directories and
    // selects the directory where it finds one
    opts['packageSelectors'] = opts.filter.map((f) => parsePackageSelector(f, process.cwd())) // tslint:disable-line
  }

  const atLeastOnePackageMatched = await recursive(allWorkspacePkgs, input, opts, cmdFullName, cmd)

  if (!atLeastOnePackageMatched) {
    logger.info({ message: `No packages matched the filters in "${workspacePrefix}"`, prefix: workspacePrefix })
    return
  }
}

export async function recursive (
  allPkgs: Array<{path: string, manifest: DependencyManifest, writeImporterManifest: (manifest: ImporterManifest) => Promise<void>}>,
  input: string[],
  opts: PnpmOptions & {
    allowNew?: boolean,
    packageSelectors?: PackageSelector[],
    ignoredPackages?: Set<string>,
    update?: boolean,
    useBetaCli?: boolean,
  },
  cmdFullName: string,
  cmd: string,
): Promise<boolean> {
  if (allPkgs.length === 0) {
    // It might make sense to throw an exception in this case
    return false
  }

  const pkgGraphResult = createPkgGraph(allPkgs)
  let pkgs: Array<{path: string, manifest: ImporterManifest, writeImporterManifest: (manifest: ImporterManifest) => Promise<void> }>
  if (opts.packageSelectors && opts.packageSelectors.length) {
    pkgGraphResult.graph = filterGraph(pkgGraphResult.graph, opts.packageSelectors)
    pkgs = allPkgs.filter(({ path }) => pkgGraphResult.graph[path])
  } else {
    pkgs = allPkgs
  }

  if (pkgs.length === 0) {
    return false
  }
  const manifestsByPath: { [path: string]: { manifest: ImporterManifest, writeImporterManifest: (manifest: ImporterManifest) => Promise<void> } } = {}
  for (const { manifest, path, writeImporterManifest } of pkgs) {
    manifestsByPath[path] = { manifest, writeImporterManifest }
  }

  scopeLogger.debug({
    selected: pkgs.length,
    total: allPkgs.length,
    workspacePrefix: opts.workspacePrefix,
  })

  const throwOnFail = throwOnCommandFail.bind(null, `pnpm recursive ${cmd}`)

  switch (cmdFullName) {
    case 'why':
    case 'list':
      await list(pkgs, input, cmd, opts as any) // tslint:disable-line:no-any
      return true
    case 'outdated':
      await outdated(pkgs, input, cmd, opts as any) // tslint:disable-line:no-any
      return true
    case 'add':
      if (!input || !input.length) {
        throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm recursive add` requires the package name')
      }
      break
  }

  const chunks = opts.sort
    ? sortPackages(pkgGraphResult.graph)
    : [Object.keys(pkgGraphResult.graph).sort()]

  switch (cmdFullName) {
    case 'test':
      throwOnFail(await run(chunks, pkgGraphResult.graph, ['test', ...input], cmd, opts as any)) // tslint:disable-line:no-any
      return true
    case 'run':
      throwOnFail(await run(chunks, pkgGraphResult.graph, input, cmd, opts as any)) // tslint:disable-line:no-any
      return true
    case 'update':
      opts = { ...opts, update: true, allowNew: false } as any // tslint:disable-line:no-any
      break
    case 'exec':
      throwOnFail(await exec(chunks, pkgGraphResult.graph, input, cmd, opts as any)) // tslint:disable-line:no-any
      return true
  }

  const store = await createStoreController(opts)

  // It is enough to save the store.json file once,
  // once all installations are done.
  // That's why saveState that is passed to the install engine
  // does nothing.
  const saveState = store.ctrl.saveState
  const storeController = {
    ...store.ctrl,
    saveState: async () => undefined,
  }

  const localPackages = opts.linkWorkspacePackages && cmdFullName !== 'unlink'
    ? arrayOfLocalPackagesToMap(allPkgs)
    : {}
  const installOpts = Object.assign(opts, {
    localPackages,
    ownLifecycleHooksStdio: 'pipe',
    peer: opts.savePeer,
    pruneLockfileImporters: (!opts.ignoredPackages || opts.ignoredPackages.size === 0)
      && pkgs.length === allPkgs.length,
    store: store.path,
    storeController,

    forceHoistPattern: typeof opts.rawLocalConfig['hoist-pattern'] !== 'undefined' || typeof opts.rawLocalConfig['hoist'] !== 'undefined',
    forceIndependentLeaves: typeof opts.rawLocalConfig['independent-leaves'] !== 'undefined',
    forceShamefullyHoist: typeof opts.rawLocalConfig['shamefully-hoist'] !== 'undefined',
  }) as InstallOptions

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const memReadLocalConfig = mem(readLocalConfig)

  async function getImporters () {
    const importers = [] as Array<{ buildIndex: number, manifest: ImporterManifest, prefix: string }>
    await Promise.all(chunks.map((prefixes: string[], buildIndex) => {
      if (opts.ignoredPackages) {
        prefixes = prefixes.filter((prefix) => !opts.ignoredPackages!.has(prefix))
      }
      return Promise.all(
        prefixes.map(async (prefix) => {
          importers.push({
            buildIndex,
            manifest: manifestsByPath[prefix].manifest,
            prefix,
          })
        })
      )
    }))
    return importers
  }

  const updateToLatest = opts.update && opts.latest
  const include = opts.include
  if (updateToLatest) {
    delete opts.include
  }

  if (cmdFullName !== 'rebuild') {
    // For a workspace with shared lockfile
    if (opts.lockfileDirectory && ['add', 'install', 'uninstall', 'update'].includes(cmdFullName)) {
      if (opts.hoistPattern) {
        logger.info({ message: 'Only the root workspace package is going to have hoisted dependencies in node_modules', prefix: opts.lockfileDirectory })
      }
      let importers = await getImporters()
      const isFromWorkspace = isSubdir.bind(null, opts.lockfileDirectory)
      importers = await pFilter(importers, async ({ prefix }: { prefix: string }) => isFromWorkspace(await fs.realpath(prefix)))
      if (importers.length === 0) return true
      const hooks = opts.ignorePnpmfile ? {} : requireHooks(opts.lockfileDirectory, opts)
      const mutation = cmdFullName === 'uninstall' ? 'uninstallSome' : (input.length === 0 && !updateToLatest ? 'install' : 'installSome')
      const writeImporterManifests = [] as Array<(manifest: ImporterManifest) => Promise<void>>
      const mutatedImporters = [] as MutatedImporter[]
      await Promise.all(importers.map(async ({ buildIndex, prefix }) => {
        const localConfig = await memReadLocalConfig(prefix)
        const { manifest, writeImporterManifest } = manifestsByPath[prefix]
        let currentInput = [...input]
        if (updateToLatest) {
          if (!currentInput || !currentInput.length) {
            currentInput = updateToLatestSpecsFromManifest(manifest, include)
          } else {
            currentInput = createLatestSpecs(currentInput, manifest)
            if (!currentInput.length) {
              installOpts.pruneLockfileImporters = false
              return
            }
          }
        }
        writeImporterManifests.push(writeImporterManifest)
        switch (mutation) {
          case 'uninstallSome':
            mutatedImporters.push({
              dependencyNames: currentInput,
              manifest,
              mutation,
              prefix,
              targetDependenciesField: getSaveType(opts),
            } as MutatedImporter)
            return
          case 'installSome':
            mutatedImporters.push({
              allowNew: cmdFullName === 'install' || cmdFullName === 'add',
              dependencySelectors: currentInput,
              manifest,
              mutation,
              peer: opts.savePeer,
              pinnedVersion: getPinnedVersion({
                saveExact: typeof localConfig.saveExact === 'boolean' ? localConfig.saveExact : opts.saveExact,
                savePrefix: typeof localConfig.savePrefix === 'string' ? localConfig.savePrefix : opts.savePrefix,
              }),
              prefix,
              targetDependenciesField: getSaveType(opts),
            } as MutatedImporter)
            return
          case 'install':
            mutatedImporters.push({
              buildIndex,
              manifest,
              mutation,
              prefix,
            } as MutatedImporter)
            return
        }
      }))
      const mutatedPkgs = await mutateModules(mutatedImporters, {
        ...installOpts,
        hooks,
        storeController: store.ctrl,
      })
      await Promise.all(
        mutatedPkgs
          .filter((mutatedPkg, index) => mutatedImporters[index].mutation !== 'install')
          .map(({ manifest, prefix }, index) => writeImporterManifests[index](manifest))
      )
      return true
    }

    let pkgPaths = chunks.length === 0
      ? chunks[0]
      : Object.keys(pkgGraphResult.graph).sort()

    const limitInstallation = pLimit(opts.workspaceConcurrency)
    await Promise.all(pkgPaths.map((prefix: string) =>
      limitInstallation(async () => {
        const hooks = opts.ignorePnpmfile ? {} : requireHooks(prefix, opts)
        try {
          if (opts.ignoredPackages && opts.ignoredPackages.has(prefix)) {
            return
          }

          const { manifest, writeImporterManifest } = manifestsByPath[prefix]
          let currentInput = [...input]
          if (updateToLatest) {
            if (!currentInput || !currentInput.length) {
              currentInput = updateToLatestSpecsFromManifest(manifest, include)
            } else {
              currentInput = createLatestSpecs(currentInput, manifest)
              if (!currentInput.length) return
            }
          }

          let action!: any // tslint:disable-line:no-any
          switch (cmdFullName) {
            case 'unlink':
              action = (currentInput.length === 0 ? unlink : unlinkPkgs.bind(null, currentInput))
              break
            case 'uninstall':
              action = (manifest: PackageManifest, opts: any) => mutateModules([ // tslint:disable-line:no-any
                {
                  dependencyNames: currentInput,
                  manifest,
                  mutation: 'uninstallSome',
                  prefix,
                },
              ], opts)
              break
            default:
              action = currentInput.length === 0
                ? install
                : (manifest: PackageManifest, opts: any) => addDependenciesToPackage(manifest, currentInput, opts) // tslint:disable-line:no-any
              break
          }

          const localConfig = await memReadLocalConfig(prefix)
          const newManifest = await action(
            manifest,
            {
              ...installOpts,
              ...localConfig,
              bin: path.join(prefix, 'node_modules', '.bin'),
              hooks,
              ignoreScripts: true,
              pinnedVersion: getPinnedVersion({
                saveExact: typeof localConfig.saveExact === 'boolean' ? localConfig.saveExact : opts.saveExact,
                savePrefix: typeof localConfig.savePrefix === 'string' ? localConfig.savePrefix : opts.savePrefix,
              }),
              prefix,
              rawConfig: {
                ...installOpts.rawConfig,
                ...localConfig,
              },
              storeController,
            },
          )
          if (action !== install) {
            await writeImporterManifest(newManifest)
          }
          result.passes++
        } catch (err) {
          logger.info(err)

          if (!opts.bail) {
            result.fails.push({
              error: err,
              message: err.message,
              prefix,
            })
            return
          }

          err['prefix'] = prefix // tslint:disable-line:no-string-literal
          throw err
        }
      }),
    ))

    await saveState()
  }

  if (
    cmdFullName === 'rebuild' ||
    !opts.lockfileOnly && !opts.ignoreScripts && (
      cmdFullName === 'add' ||
      cmdFullName === 'install' ||
      cmdFullName === 'update' ||
      cmdFullName === 'unlink'
    )
  ) {
    const action = (
      cmdFullName !== 'rebuild' || input.length === 0
      ? rebuild
      : (importers: any, opts: any) => rebuildPkgs(importers, input, opts) // tslint:disable-line
    )
    if (opts.lockfileDirectory) {
      const importers = await getImporters()
      await action(
        importers,
        {
          ...installOpts,
          pending: cmdFullName !== 'rebuild' || opts.pending === true,
        },
      )
      return true
    }
    const limitRebuild = pLimit(opts.workspaceConcurrency)
    for (const chunk of chunks) {
      await Promise.all(chunk.map((prefix: string) =>
        limitRebuild(async () => {
          try {
            if (opts.ignoredPackages && opts.ignoredPackages.has(prefix)) {
              return
            }
            const localConfig = await memReadLocalConfig(prefix)
            await action(
              [
                {
                  buildIndex: 0,
                  manifest: manifestsByPath[prefix].manifest,
                  prefix,
                },
              ],
              {
                ...installOpts,
                ...localConfig,
                bin: path.join(prefix, 'node_modules', '.bin'),
                pending: cmdFullName !== 'rebuild' || opts.pending === true,
                prefix,
                rawConfig: {
                  ...installOpts.rawConfig,
                  ...localConfig,
                },
              },
            )
            result.passes++
          } catch (err) {
            logger.info(err)

            if (!opts.bail) {
              result.fails.push({
                error: err,
                message: err.message,
                prefix,
              })
              return
            }

            err['prefix'] = prefix // tslint:disable-line:no-string-literal
            throw err
          }
        }),
      ))
    }
  }

  throwOnFail(result)

  return true
}

async function unlink (manifest: ImporterManifest, opts: any) { // tslint:disable-line:no-any
  return mutateModules(
    [
      {
        manifest,
        mutation: 'unlink',
        prefix: opts.prefix,
      },
    ],
    opts,
  )
}

async function unlinkPkgs (dependencyNames: string[], manifest: ImporterManifest, opts: any) { // tslint:disable-line:no-any
  return mutateModules(
    [
      {
        dependencyNames,
        manifest,
        mutation: 'unlinkSome',
        prefix: opts.prefix,
      },
    ],
    opts,
  )
}

function sortPackages<T> (pkgGraph: {[nodeId: string]: PackageNode<T>}): string[][] {
  const keys = Object.keys(pkgGraph)
  const setOfKeys = new Set(keys)
  const graph = new Map(
    keys.map((pkgPath) => [
      pkgPath,
      pkgGraph[pkgPath].dependencies.filter(
        /* remove cycles of length 1 (ie., package 'a' depends on 'a').  They
        confuse the graph-sequencer, but can be ignored when ordering packages
        topologically.

        See the following example where 'b' and 'c' depend on themselves:

          graphSequencer({graph: new Map([
            ['a', ['b', 'c']],
            ['b', ['b']],
            ['c', ['b', 'c']]]
          ),
          groups: [['a', 'b', 'c']]})

        returns chunks:

            [['b'],['a'],['c']]

        But both 'b' and 'c' should be executed _before_ 'a', because 'a' depends on
        them.  It works (and is considered 'safe' if we run:)

          graphSequencer({graph: new Map([
            ['a', ['b', 'c']],
            ['b', []],
            ['c', ['b']]]
          ), groups: [['a', 'b', 'c']]})

        returning:

            [['b'], ['c'], ['a']]

        */
        d => d !== pkgPath &&
        /* remove unused dependencies that we can ignore due to a filter expression.

        Again, the graph sequencer used to behave weirdly in the following edge case:

          graphSequencer({graph: new Map([
            ['a', ['b', 'c']],
            ['d', ['a']],
            ['e', ['a', 'b', 'c']]]
          ),
          groups: [['a', 'e', 'e']]})

        returns chunks:

            [['d'],['a'],['e']]

        But we really want 'a' to be executed first.
        */
        setOfKeys.has(d))]
    ) as Array<[string, string[]]>,
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [keys],
  })
  return graphSequencerResult.chunks
}

async function readLocalConfig (prefix: string) {
  try {
    const ini = await readIniFile(path.join(prefix, '.npmrc')) as { [key: string]: string }
    const config = camelcaseKeys(ini) as ({ [key: string]: string } & { hoist?: boolean })
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
