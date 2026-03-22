import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { checkPeerDependencies } from '@pnpm/deps.inspection.list'
import { renderPeerIssues } from '@pnpm/installing.render-peer-issues'
import type { PeerDependencyIssuesByProjects } from '@pnpm/types'
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
          {
            description: 'Perform command on every package in subdirectories \
or on every workspace package, when executed inside a workspace.',
            name: '--recursive',
            shortAlias: '-r',
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
| 'peerDependencyRules'
| 'selectedProjectsGraph'
> & Partial<Pick<Config, 'cliOptions'>> & {
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
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const projectPaths = opts.recursive && opts.selectedProjectsGraph
    ? Object.keys(opts.selectedProjectsGraph)
    : [opts.dir]

  const issues = await checkPeerDependencies(projectPaths, {
    lockfileDir,
    checkWantedLockfileOnly: opts.lockfileOnly,
    modulesDir: opts.modulesDir,
    peerDependencyRules: opts.peerDependencyRules,
  })

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

  const rendered = renderPeerIssues(issues)
  return {
    output: rendered ? `Issues with peer dependencies found\n${rendered}` : '',
    exitCode: 1,
  }
}

function hasNoIssues (issues: PeerDependencyIssuesByProjects): boolean {
  return Object.values(issues).every(
    (projectIssues) =>
      isEmpty(projectIssues.bad) &&
      isEmpty(projectIssues.missing)
  )
}
