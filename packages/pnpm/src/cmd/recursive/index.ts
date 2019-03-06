import { stageLogger } from '@pnpm/core-loggers'
import logger from '@pnpm/logger'
import { PackageJson } from '@pnpm/types'
import { getSaveType } from '@pnpm/utils'
import camelcaseKeys = require('camelcase-keys')
import graphSequencer = require('graph-sequencer')
import isSubdir = require('is-subdir')
import mem = require('mem')
import fs = require('mz/fs')
import pFilter = require('p-filter')
import pLimit = require('p-limit')
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
  uninstall,
} from 'supi'
import createStoreController from '../../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../../findWorkspacePackages'
import getCommandFullName from '../../getCommandFullName'
import getPinnedVersion from '../../getPinnedVersion'
import { scopeLogger } from '../../loggers'
import parsePackageSelector, { PackageSelector } from '../../parsePackageSelectors'
import requireHooks from '../../requireHooks'
import { PnpmOptions } from '../../types'
import help from '../help'
import exec from './exec'
import { filterGraph } from './filter'
import list from './list'
import outdated from './outdated'
import RecursiveSummary, { throwOnCommandFail } from './recursiveSummary'
import run from './run'

const supportedRecursiveCommands = new Set([
  'install',
  'uninstall',
  'update',
  'unlink',
  'list',
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
    const err = new Error('Workspace concurrency should be at least 1')
    err['code'] = 'ERR_PNPM_INVALID_WORKSPACE_CONCURRENCY' // tslint:disable-line:no-string-literal
    throw err
  }

  const cmd = input.shift()
  if (!cmd) {
    help(['recursive'])
    return
  }
  const cmdFullName = getCommandFullName(cmd)
  if (!supportedRecursiveCommands.has(cmdFullName)) {
    help(['recursive'])
    const err = new Error(`"recursive ${cmdFullName}" is not a pnpm command. See "pnpm help recursive".`)
    err['code'] = 'ERR_PNPM_INVALID_RECURSIVE_COMMAND' // tslint:disable-line:no-string-literal
    throw err
  }

  const workspacePrefix = opts.workspacePrefix || process.cwd()
  const allWorkspacePkgs = await findWorkspacePackages(workspacePrefix)

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
  allPkgs: Array<{path: string, manifest: PackageJson}>,
  input: string[],
  opts: PnpmOptions & {
    allowNew?: boolean,
    packageSelectors?: PackageSelector[],
    ignoredPackages?: Set<string>,
  },
  cmdFullName: string,
  cmd: string,
): Promise<boolean> {
  if (allPkgs.length === 0) {
    // It might make sense to throw an exception in this case
    return false
  }

  const pkgGraphResult = createPkgGraph(allPkgs)
  let pkgs: Array<{path: string, manifest: PackageJson}>
  if (opts.packageSelectors && opts.packageSelectors.length) {
    pkgGraphResult.graph = filterGraph(pkgGraphResult.graph, opts.packageSelectors)
    pkgs = allPkgs.filter((pkg: {path: string}) => pkgGraphResult.graph[pkg.path])
  } else {
    pkgs = allPkgs
  }

  if (pkgs.length === 0) {
    return false
  }

  scopeLogger.debug({
    selected: pkgs.length,
    total: allPkgs.length,
    workspacePrefix: opts.workspacePrefix,
  })

  const throwOnFail = throwOnCommandFail.bind(null, `pnpm recursive ${cmd}`)

  switch (cmdFullName) {
    case 'list':
      await list(pkgs, input, cmd, opts as any) // tslint:disable-line:no-any
      return true
    case 'outdated':
      await outdated(pkgs, input, cmd, opts as any) // tslint:disable-line:no-any
      return true
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
    pruneLockfileImporters: (!opts.ignoredPackages || opts.ignoredPackages.size === 0)
      && pkgs.length === allPkgs.length,
    store: store.path,
    storeController,
    targetDependenciesField: getSaveType(opts),
  }) as InstallOptions

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const memReadLocalConfigs = mem(readLocalConfigs)

  function getImporters () {
    const importers = [] as Array<{ buildIndex: number, prefix: string }>
    chunks.forEach((prefixes: string[], buildIndex) => {
      if (opts.ignoredPackages) {
        prefixes = prefixes.filter((prefix) => !opts.ignoredPackages!.has(prefix))
      }
      prefixes.forEach((prefix) => {
        importers.push({ buildIndex, prefix })
      })
    })
    return importers
  }

  if (cmdFullName !== 'rebuild') {
    if (opts.lockfileDirectory && ['install', 'uninstall', 'update'].indexOf(cmdFullName) !== -1) {
      let importers = getImporters()
      const isFromWorkspace = isSubdir.bind(null, opts.lockfileDirectory)
      importers = await pFilter(importers, async ({ prefix }: { prefix: string }) => isFromWorkspace(await fs.realpath(prefix)))
      if (importers.length === 0) return true
      const hooks = opts.ignorePnpmfile ? {} : requireHooks(opts.lockfileDirectory, opts)
      const mutation = cmdFullName === 'uninstall' ? 'uninstallSome' : (input.length === 0 ? 'install' : 'installSome')
      const mutatedImporters = await Promise.all<MutatedImporter>(importers.map(async ({ buildIndex, prefix }) => {
        const localConfigs = await memReadLocalConfigs(prefix)
        const shamefullyFlatten = typeof localConfigs.shamefullyFlatten === 'boolean'
          ? localConfigs.shamefullyFlatten
          : opts.shamefullyFlatten
        switch (mutation) {
          case 'uninstallSome':
            return {
              dependencyNames: input,
              mutation,
              prefix,
              shamefullyFlatten,
              targetDependenciesField: getSaveType(installOpts),
            } as MutatedImporter
          case 'installSome':
            return {
              allowNew: cmdFullName === 'install',
              dependencySelectors: input,
              mutation,
              pinnedVersion: getPinnedVersion({
                saveExact: typeof localConfigs.saveExact === 'boolean' ? localConfigs.saveExact : opts.saveExact,
                savePrefix: typeof localConfigs.savePrefix === 'string' ? localConfigs.savePrefix : opts.savePrefix,
              }),
              prefix,
              shamefullyFlatten,
              targetDependenciesField: getSaveType(installOpts),
            } as MutatedImporter
          case 'install':
            return {
              buildIndex,
              mutation,
              prefix,
              shamefullyFlatten,
            } as MutatedImporter
        }
      }))
      await mutateModules(mutatedImporters, {
        ...installOpts,
        hooks,
        storeController: store.ctrl,
      })
      return true
    }

    let pkgPaths = chunks.length === 0
      ? chunks[0]
      : Object.keys(pkgGraphResult.graph).sort()

    let action!: any // tslint:disable-line:no-any
    switch (cmdFullName) {
      case 'unlink':
        action = (input.length === 0 ? unlink : unlinkPkgs.bind(null, input))
        break
      case 'uninstall':
        action = uninstall.bind(null, input)
        break
      default:
        action = input.length === 0 ? install : addDependenciesToPackage.bind(null, input)
        break
    }
    const limitInstallation = pLimit(opts.workspaceConcurrency)
    await Promise.all(pkgPaths.map((prefix: string) =>
      limitInstallation(async () => {
        const hooks = opts.ignorePnpmfile ? {} : requireHooks(prefix, opts)
        try {
          if (opts.ignoredPackages && opts.ignoredPackages.has(prefix)) {
            return
          }
          const localConfigs = await memReadLocalConfigs(prefix)
          await action({
            ...installOpts,
            ...localConfigs,
            bin: path.join(prefix, 'node_modules', '.bin'),
            hooks,
            ignoreScripts: true,
            prefix,
            rawNpmConfig: {
              ...installOpts.rawNpmConfig,
              ...localConfigs.rawNpmConfig,
            },
            storeController,
          })
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
    !opts.lockfileOnly && !opts.ignoreScripts && (cmdFullName === 'install' || cmdFullName === 'update' || cmdFullName === 'unlink')
  ) {
    const action = (
      cmdFullName !== 'rebuild' || input.length === 0
      ? rebuild
      : (importers: any, opts: any) => rebuildPkgs(importers, input, opts) // tslint:disable-line
    )
    if (opts.lockfileDirectory) {
      const importers = getImporters()
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
            const localConfigs = await memReadLocalConfigs(prefix)
            await action(
              [{ buildIndex: 0, prefix }],
              {
                ...installOpts,
                ...localConfigs,
                bin: path.join(prefix, 'node_modules', '.bin'),
                pending: cmdFullName !== 'rebuild' || opts.pending === true,
                prefix,
                rawNpmConfig: {
                  ...installOpts.rawNpmConfig,
                  ...localConfigs.rawNpmConfig,
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

function unlink (opts: any) { // tslint:disable-line:no-any
  return mutateModules(
    [
      {
        mutation: 'unlink',
        prefix: opts.prefix,
      },
    ],
    opts,
  )
}

function unlinkPkgs (dependencyNames: string[], opts: any) { // tslint:disable-line:no-any
  return mutateModules(
    [
      {
        dependencyNames,
        mutation: 'unlinkSome',
        prefix: opts.prefix,
      },
    ],
    opts,
  )
}

function sortPackages (pkgGraph: {[nodeId: string]: PackageNode}): string[][] {
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

async function readLocalConfigs (prefix: string) {
  try {
    const ini = await readIniFile(path.join(prefix, '.npmrc'))
    return camelcaseKeys(ini)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
    return {}
  }
}
