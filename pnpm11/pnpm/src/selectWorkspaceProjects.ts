import fs from 'node:fs'
import path from 'node:path'

import type { Config, ConfigContext } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { filterProjectsFromDir, type WorkspaceFilter } from '@pnpm/workspace.projects-filter'
import { isEmpty } from 'ramda'

import { recursiveByDefaultCommands } from './cmd/index.js'

export interface CommandInvocation {
  cmd: string | null
  cliParams: string[]
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- option values have command-specific types
  cliOptions: Record<string, any>
  workspaceDir: string | undefined
}

/**
 * Turns on recursive mode for the commands that are recursive by default
 * inside a workspace. `list` run from a workspace subdirectory without a
 * filter stays scoped to the current project.
 */
export function enableRecursiveByDefault (invocation: CommandInvocation, config: Config): void {
  const { cmd, cliOptions, workspaceDir } = invocation
  const hasFilter = Boolean(config.filter?.length || config.filterProd?.length)
  const isWorkspaceSubdirectory = typeof workspaceDir === 'string' &&
    getRealPathSync(config.dir) !== getRealPathSync(workspaceDir)
  const isListCommand = cmd === 'list' || cmd === 'll'
  const hasExplicitRecursive = cliOptions['recursive'] === true

  if (
    cmd == null || !recursiveByDefaultCommands.has(cmd) ||
    typeof workspaceDir !== 'string' ||
    (isListCommand && isWorkspaceSubdirectory && !hasFilter)
  ) return

  cliOptions['recursive'] = true
  config.recursive = true

  if (hasExplicitRecursive) {
    config.recursiveInstall = true
  } else if (!config.recursiveInstall && !config.filter && !config.filterProd) {
    config.filter = ['{.}...']
  }
}

/**
 * Selects the workspace projects a recursive command runs on and stores them
 * in `context`. Returns `false`, having set `process.exitCode`, when the
 * command should not run because nothing was selected.
 */
export async function selectWorkspaceProjects (
  invocation: CommandInvocation,
  opts: { config: Config, context: ConfigContext, printLogs: boolean }
): Promise<boolean> {
  const { config, context, printLogs } = opts
  config.recursive = true
  const wsDir = invocation.workspaceDir ?? process.cwd()

  config.filter = config.filter ?? []
  config.filterProd = config.filterProd ?? []

  const filters = buildWorkspaceFilters(config, {
    ...invocation,
    wsDir,
    filter: config.filter,
    filterProd: config.filterProd,
  })
  const filterResults = await filterProjectsFromDir(wsDir, filters, createProjectsFilterOptions(config, wsDir))

  if (filterResults.allProjects.length === 0) {
    if (printLogs) {
      console.log(`No projects found in "${wsDir}"`)
    }
    process.exitCode = config.failIfNoMatch ? 1 : 0
    return false
  }
  context.allProjectsGraph = filterResults.allProjectsGraph
  context.selectedProjectsGraph = filterResults.selectedProjectsGraph
  context.prodAllProjectsGraph = filterResults.prodAllProjectsGraph
  context.prodOnlySelectedProjectDirs = filterResults.prodOnlySelectedProjectDirs
  if (isEmpty(context.selectedProjectsGraph) && !reportEmptySelection(invocation.cmd, { config, printLogs, wsDir })) {
    return false
  }
  if (filterResults.unmatchedFilters.length !== 0 && printLogs) {
    console.log(`No projects matched the filters "${filterResults.unmatchedFilters.join(', ')}" in "${wsDir}"`)
  }
  context.allProjects = filterResults.allProjects
  config.workspaceDir = wsDir
  return true
}

function buildWorkspaceFilters (
  config: Config,
  scope: CommandInvocation & { wsDir: string, filter: string[], filterProd: string[] }
): WorkspaceFilter[] {
  const { cmd, workspaceDir, wsDir } = scope
  const filters: WorkspaceFilter[] = [
    ...scope.filter.map((filter) => ({ filter, followProdDepsOnly: false })),
    ...scope.filterProd.map((filter) => ({ filter, followProdDepsOnly: true })),
  ]
  const relativeWSDirPath = () => path.relative(process.cwd(), wsDir) || '.'
  // Both of the selectors below are pnpm's own; the user did not write
  // them. Each has to mean "the project whose directory is the workspace
  // root", which only glob matching says. Left to follow the pass,
  // `legacyDirFiltering`'s subtree matching would read them as "every
  // project below the root" — including the root's descendants instead
  // of the root, and excluding them instead of it.
  const cmdSkipsWorkspaceRootByDefault = cmd === 'run' || cmd === 'exec' || cmd === 'add' || cmd === 'test'
  if (config.workspaceRoot) {
    filters.push({ filter: `{${relativeWSDirPath()}}`, followProdDepsOnly: Boolean(scope.filterProd.length), useGlobDirFiltering: true })
  } else if (
    !filters.some(({ filter }) => !filter.startsWith('!')) &&
    workspaceDir &&
    config.workspacePackagePatterns &&
    !isRootOnlyPatterns(config.workspacePackagePatterns) &&
    !config.includeWorkspaceRoot &&
    cmdSkipsWorkspaceRootByDefault
  ) {
    filters.push({ filter: `!{${relativeWSDirPath()}}`, followProdDepsOnly: Boolean(scope.filterProd.length), useGlobDirFiltering: true })
  }
  return filters
}

function createProjectsFilterOptions (config: Config, wsDir: string): Parameters<typeof filterProjectsFromDir>[2] {
  return {
    catalogs: config.catalogs,
    engineStrict: config.engineStrict,
    nodeVersion: config.nodeVersion,
    patterns: config.workspacePackagePatterns,
    modulesDir: config.modulesDir,
    modulesDirsByProjectName: config.modulesDirsByProjectName,
    linkWorkspacePackages: !!config.linkWorkspacePackages,
    prefix: process.cwd(),
    workspaceDir: wsDir,
    testPattern: config.testPattern,
    changedFilesIgnorePattern: config.changedFilesIgnorePattern,
    useGlobDirFiltering: !config.legacyDirFiltering,
    sharedWorkspaceLockfile: config.sharedWorkspaceLockfile,
  }
}

/**
 * Reports that the filters selected no project. Returns whether the command
 * still runs, setting `process.exitCode` when it does not.
 */
function reportEmptySelection (
  cmd: string | null,
  opts: { config: Config, printLogs: boolean, wsDir: string }
): boolean {
  if (opts.printLogs) {
    console.log(`No projects matched the filters in "${opts.wsDir}"`)
  }
  if (opts.config.failIfNoMatch) {
    process.exitCode = 1
    return false
  }
  // "change" operates on the whole workspace through allProjects, so an
  // empty selection (e.g. run from a directory without packages beneath
  // it) must not skip the command.
  if (cmd !== 'list' && cmd !== 'change') {
    process.exitCode = 0
    return false
  }
  return true
}

function isRootOnlyPatterns (patterns: string[]): boolean {
  return patterns.length === 1 && patterns[0] === '.'
}

function getRealPathSync (dir: string): string {
  const resolved = path.resolve(dir)
  try {
    return fs.realpathSync.native(resolved)
  } catch (err: unknown) {
    throw new PnpmError(
      'WORKSPACE_DIR_NOT_FOUND',
      `Failed to resolve real path for "${resolved}"`,
      { cause: err }
    )
  }
}
