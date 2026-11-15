import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, createProjectModulesDirResolver, types as allTypes } from '@pnpm/config.reader'
import { checkPeerDependencies } from '@pnpm/deps.inspection.peers-checker'
import { renderPeerIssues } from '@pnpm/deps.inspection.peers-issues-renderer'
import { getLockfileImporterId } from '@pnpm/lockfile.fs'
import type { PeerDependencyIssues, PeerDependencyIssuesByProjects, ProjectRootDir } from '@pnpm/types'
import { isEmpty, pick } from 'ramda'
import { renderHelp } from 'render-help'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'json',
    'lockfile-only',
  ], allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
})

export const shorthands = {}

export const commandNames = ['peers']

export const recursiveByDefault = true

export function help (): string {
  return renderHelp({
    description: 'Commands for inspecting peer dependency relationships.',
    descriptionLists: [
      {
        title: 'Commands',
        list: [
          {
            description: 'Checks for unmet or missing peer dependency issues by reading the lockfile. \
Exits with a non-zero exit code when issues are found.',
            name: 'check',
          },
        ],
      },
      {
        title: 'Options',
        list: [
          {
            description: 'Show information in JSON format',
            name: '--json',
          },
          {
            description: 'Check the lockfile only, without reading node_modules.',
            name: '--lockfile-only',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('peers'),
    usages: [
      'pnpm peers <command>',
    ],
  })
}

export type PeersCommandOptions = Pick<Config,
| 'dir'
| 'modulesDir'
| 'packageConfigs'
| 'peerDependencyRules'
| 'sharedWorkspaceLockfile'
| 'workspaceDir'
> & Pick<ConfigContext, 'selectedProjectsGraph'>
& Partial<Pick<ConfigContext, 'cliOptions'>> & {
  json?: boolean
  lockfileDir?: string
  lockfileOnly?: boolean
  recursive?: boolean
}

export async function handler (
  opts: PeersCommandOptions,
  params: string[]
): Promise<string | { output: string, exitCode: number }> {
  switch (params[0]) {
    case 'check':
    case undefined:
      return checkCmd(opts)
    default:
      return { output: help(), exitCode: 1 }
  }
}

async function checkCmd (
  opts: PeersCommandOptions
): Promise<{ output: string, exitCode: number }> {
  const projectPaths = opts.recursive && opts.selectedProjectsGraph
    ? Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]
    : [opts.dir as ProjectRootDir]

  const issues = await collectPeerIssues(opts, projectPaths)

  const noIssues = hasNoIssues(issues)
  const json = opts.json ?? opts.cliOptions?.['json'] as boolean | undefined

  if (json) {
    return {
      output: JSON.stringify(issues, null, 2),
      exitCode: noIssues ? 0 : 1,
    }
  }

  if (noIssues) {
    return { output: 'No peer dependency issues found', exitCode: 0 }
  }

  return {
    output: `Issues with peer dependencies found\n\n${renderPeerIssues(issues)}`,
    exitCode: 1,
  }
}

async function collectPeerIssues (
  opts: PeersCommandOptions,
  projectPaths: ProjectRootDir[]
): Promise<PeerDependencyIssuesByProjects> {
  if (opts.lockfileDir || opts.sharedWorkspaceLockfile !== false || !opts.workspaceDir) {
    return checkPeerDependencies(projectPaths, {
      lockfileDir: opts.lockfileDir ?? opts.dir,
      checkWantedLockfileOnly: opts.lockfileOnly,
      modulesDir: opts.modulesDir,
      peerDependencyRules: opts.peerDependencyRules,
    })
  }
  return collectDedicatedPeerIssues(opts, opts.workspaceDir, projectPaths)
}

async function collectDedicatedPeerIssues (
  opts: PeersCommandOptions,
  workspaceDir: string,
  projectPaths: ProjectRootDir[]
): Promise<PeerDependencyIssuesByProjects> {
  const resolveModulesDir = createProjectModulesDirResolver(opts)
  const entries: Array<readonly [string, PeerDependencyIssues]> = []
  for (const projectPath of projectPaths) {
    const projectName = opts.selectedProjectsGraph?.[projectPath]?.package.manifest.name
    // eslint-disable-next-line no-await-in-loop -- reading per-project lockfiles one at a time bounds open file descriptors in large workspaces
    const projectIssues = await checkPeerDependencies([projectPath], {
      lockfileDir: projectPath,
      checkWantedLockfileOnly: opts.lockfileOnly,
      modulesDir: resolveModulesDir(projectName),
      peerDependencyRules: opts.peerDependencyRules,
    })
    if (projectIssues['.'] != null) {
      const key = opts.recursive ? getLockfileImporterId(workspaceDir, projectPath) : '.'
      entries.push([key, projectIssues['.']])
    }
  }
  entries.sort(([keyA], [keyB]) => (keyA < keyB ? -1 : keyA > keyB ? 1 : 0))
  return Object.fromEntries(entries)
}

function hasNoIssues (issues: PeerDependencyIssuesByProjects): boolean {
  return Object.values(issues).every(
    (projectIssues) =>
      isEmpty(projectIssues.bad) &&
      isEmpty(projectIssues.missing)
  )
}
