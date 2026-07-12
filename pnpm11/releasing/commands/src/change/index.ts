import { checkbox, input, select } from '@inquirer/prompts'
import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import {
  assembleReleasePlan,
  BUMP_TYPES,
  type ChangeIntent,
  type IntentBumpType,
  readChangeIntents,
  readLedger,
  type ReleasePlan,
  type WorkspaceProject,
  writeChangeIntent,
} from '@pnpm/releasing.versioning'
import type { Project, VersioningSettings } from '@pnpm/types'
import { renderHelp } from 'render-help'
import { valid } from 'semver'

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    bump: String,
    summary: String,
    recursive: Boolean,
  }
}

export const commandNames = ['change']

export function help (): string {
  return renderHelp({
    description: 'Records a change intent: which packages a change affects, the bump type for each, and a summary that becomes the changelog entry. The intent file is written to .changeset/ in the changesets format.',
    usages: [
      'pnpm change [--bump <type>] [--summary <text>] [<pkg>...]',
      'pnpm change status',
    ],
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: `Bump type for the named packages: ${BUMP_TYPES.join(', ')}. "none" records an explicit decline — the change needs no release`,
            name: '--bump <type>',
          },
          {
            description: 'The summary for the changelog entry. Runs non-interactively when given together with package names',
            name: '--summary <text>',
          },
        ],
      },
    ],
  })
}

export type ChangeCommandOptions = Pick<Config,
| 'dir'
| 'versioning'
| 'workspaceDir'
> & {
  allProjects?: Project[]
  bump?: string
  summary?: string
}

export async function handler (opts: ChangeCommandOptions, params: string[]): Promise<string> {
  const workspaceDir = opts.workspaceDir
  if (!workspaceDir) {
    throw new PnpmError('WORKSPACE_ONLY', 'pnpm change is only supported in a workspace')
  }
  // Only the exact no-option invocation is the status form, so a package
  // that happens to be named "status" stays recordable.
  if (params.length === 1 && params[0] === 'status' && opts.bump == null && opts.summary == null) {
    return renderStatus(workspaceDir, opts)
  }
  return recordChange(workspaceDir, opts, params)
}

async function recordChange (workspaceDir: string, opts: ChangeCommandOptions, params: string[]): Promise<string> {
  const releasablePkgNames = getReleasablePkgNames(opts.allProjects ?? [], opts.versioning)
  if (releasablePkgNames.length === 0) {
    throw new PnpmError('VERSIONING_NO_PACKAGES', 'No releasable packages found in this workspace')
  }

  for (const pkgName of params) {
    if (!releasablePkgNames.includes(pkgName)) {
      throw new PnpmError('VERSIONING_UNKNOWN_PACKAGE', `${pkgName} is not a releasable package of this workspace`)
    }
  }

  if (opts.bump != null && !(BUMP_TYPES as readonly string[]).includes(opts.bump)) {
    throw new PnpmError('VERSIONING_INVALID_BUMP', `Invalid bump type: ${opts.bump}. Expected one of ${BUMP_TYPES.join(', ')}`)
  }

  const pkgNames = params.length > 0
    ? params
    : await checkbox({
      message: 'Which packages does this change affect?',
      choices: releasablePkgNames.map((name) => ({ value: name })),
      required: true,
    })

  const releases: Record<string, IntentBumpType> = {}
  for (const pkgName of pkgNames) {
    releases[pkgName] = (opts.bump as IntentBumpType | undefined) ??
      // eslint-disable-next-line no-await-in-loop
      await select<IntentBumpType>({
        message: `Bump type for ${pkgName}`,
        choices: BUMP_TYPES.map((bumpType) => ({ value: bumpType })).reverse(),
        default: 'patch',
      })
  }

  const summary = opts.summary ??
    await input({ message: 'Summary of the change (becomes the changelog entry):', required: true })

  const id = await writeChangeIntent(workspaceDir, { releases, summary })
  return `Recorded change intent .changeset/${id}.md`
}

async function renderStatus (workspaceDir: string, opts: ChangeCommandOptions): Promise<string> {
  const intents = await readChangeIntents(workspaceDir)
  const ledger = await readLedger(workspaceDir)
  const plan = assembleReleasePlan({
    projects: toWorkspaceProjects(opts.allProjects ?? []),
    intents,
    ledger,
    versioning: opts.versioning,
  })
  if (plan.releases.length === 0) {
    return 'No pending changes.'
  }
  const consumedIds = new Set(plan.releases.flatMap((release) => release.intents.map((intent) => intent.id)))
  let output = 'Pending change intents:\n'
  for (const intent of intents.filter(({ id }) => consumedIds.has(id))) {
    output += `  .changeset/${intent.id}.md\n`
  }
  output += '\n'
  output += renderReleasePlan(plan)
  return output
}

export function renderReleasePlan (plan: ReleasePlan): string {
  let output = 'Release plan:\n'
  for (const release of plan.releases) {
    output += `  ${release.name}: ${release.currentVersion} → ${release.newVersion} (${release.bumpType}, via ${release.causes.join('+')})\n`
  }
  return output
}

export function getReleasablePkgNames (allProjects: Array<Pick<Project, 'manifest'>>, versioning?: VersioningSettings): string[] {
  const ignored = new Set(versioning?.ignore ?? [])
  return allProjects
    .filter(({ manifest }) =>
      manifest.name != null &&
      manifest.version != null &&
      valid(manifest.version) != null &&
      !ignored.has(manifest.name))
    .map((project) => project.manifest.name!)
    .sort()
}

export function toWorkspaceProjects (allProjects: Array<Pick<Project, 'manifest' | 'rootDir'>>): WorkspaceProject[] {
  return allProjects.map((project) => ({ rootDir: project.rootDir, manifest: project.manifest }))
}

export type { ChangeIntent }

export const change = {
  handler,
  help,
  commandNames,
  cliOptionsTypes,
  rcOptionsTypes,
  recursiveByDefault: true,
}
