import { checkbox, input, Separator } from '@inquirer/prompts'
import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import {
  assembleReleasePlan,
  BUMP_TYPES,
  type ChangeIntent,
  indexProjectRefs,
  type IntentBumpType,
  readChangeIntents,
  readLedger,
  type ReleasePlan,
  toProjectDir,
  type WorkspaceProject,
  writeChangeIntent,
} from '@pnpm/releasing.versioning'
import type { Project, VersioningSettings } from '@pnpm/types'
import { getChangedProjects } from '@pnpm/workspace.projects-filter'
import { safeExeca as execa } from 'execa'
import { renderHelp } from 'render-help'
import { valid } from 'semver'

import { resolveUnpublishedDirs, type UnpublishedProbeOptions } from '../resolveUnpublishedDirs.js'

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
| 'changedFilesIgnorePattern'
| 'dir'
| 'testPattern'
| 'versioning'
| 'workspaceDir'
> & UnpublishedProbeOptions & {
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
  const releasable = getReleasableProjects(opts.allProjects ?? [], workspaceDir, opts.versioning)
  if (releasable.length === 0) {
    throw new PnpmError('VERSIONING_NO_PACKAGES', 'No releasable packages found in this workspace')
  }
  const releasableDirs = new Set(releasable.map((project) => project.dir))
  const refs = indexProjectRefs(opts.allProjects ?? [], workspaceDir)

  for (const ref of params) {
    const dirs = refs.refToDirs(ref)
    if (dirs.length > 1) {
      throw new PnpmError(
        'VERSIONING_AMBIGUOUS_PACKAGE',
        `${ref} matches multiple workspace projects: ${dirs.map((dir) => `./${dir}`).join(', ')}. Reference the project by directory instead.`
      )
    }
    if (dirs.length === 0 || !releasableDirs.has(dirs[0])) {
      throw new PnpmError('VERSIONING_UNKNOWN_PACKAGE', `${ref} is not a releasable package of this workspace`)
    }
  }

  if (opts.bump != null && !(BUMP_TYPES as readonly string[]).includes(opts.bump)) {
    throw new PnpmError('VERSIONING_INVALID_BUMP', `Invalid bump type: ${opts.bump}. Expected one of ${BUMP_TYPES.join(', ')}`)
  }

  const pkgRefs = params.length > 0
    ? params
    : await promptForPackages(releasable, workspaceDir, opts)

  const releases = opts.bump != null
    ? Object.fromEntries(pkgRefs.map((ref) => [ref, opts.bump as IntentBumpType]))
    : await promptBumpTypes(pkgRefs)

  const summary = opts.summary ??
    await input({ message: 'Summary of the change (becomes the changelog entry):', required: true })

  const id = await writeChangeIntent(workspaceDir, { releases, summary })
  return `Recorded change intent .changeset/${id}.md`
}

/**
 * The affected-packages picker, changesets-style: the packages whose
 * directories the branch touched are grouped first (under a "changed
 * packages" heading) and preselected, the rest listed below under "unchanged
 * packages". A name shared by several projects is offered under its directory
 * reference so the written intent stays unambiguous. Change detection is
 * best-effort — outside a git repo, or when no base branch can be found, the
 * list falls back to a flat, unselected picker.
 */
async function promptForPackages (
  releasable: ReleasableProject[],
  workspaceDir: string,
  opts: ChangeCommandOptions
): Promise<string[]> {
  const changedDirs = await detectChangedDirs(releasable, workspaceDir, opts)
  const label = (project: ReleasableProject): string =>
    project.ref === project.name ? project.name : `${project.name} (./${project.dir})`

  let choices: Array<Separator | { value: string, name: string, checked?: boolean }>
  if (changedDirs.size > 0) {
    const changed = releasable.filter((project) => changedDirs.has(project.dir))
    const unchanged = releasable.filter((project) => !changedDirs.has(project.dir))
    choices = [
      new Separator('changed packages'),
      ...changed.map((project) => ({ value: project.ref, name: label(project), checked: true })),
      ...(unchanged.length > 0 ? [new Separator('unchanged packages')] : []),
      ...unchanged.map((project) => ({ value: project.ref, name: label(project) })),
    ]
  } else {
    choices = releasable.map((project) => ({ value: project.ref, name: label(project) }))
  }

  return checkbox<string>({
    message: 'Which packages does this change affect?',
    choices,
    required: true,
  })
}

/**
 * The workspace-relative directories the current branch changed, relative to
 * the base branch, using the same detection behind `--filter="[<ref>]"`.
 * Returns an empty set on any failure so the picker degrades to a flat list.
 */
async function detectChangedDirs (
  releasable: ReleasableProject[],
  workspaceDir: string,
  opts: ChangeCommandOptions
): Promise<Set<string>> {
  const baseCommit = await detectBaseCommit(workspaceDir)
  if (baseCommit == null) return new Set()
  try {
    const projectDirs = (opts.allProjects ?? []).map((project) => project.rootDir)
    const [changedRootDirs] = await getChangedProjects(projectDirs, baseCommit, {
      workspaceDir,
      testPattern: opts.testPattern,
      changedFilesIgnorePattern: opts.changedFilesIgnorePattern,
    })
    const releasableDirs = new Set(releasable.map((project) => project.dir))
    return new Set(
      changedRootDirs
        .map((rootDir) => toProjectDir(workspaceDir, rootDir))
        .filter((dir) => releasableDirs.has(dir))
    )
  } catch {
    return new Set()
  }
}

/** The merge-base of HEAD with the default branch, or `undefined`. */
async function detectBaseCommit (cwd: string): Promise<string | undefined> {
  for (const branch of ['main', 'master']) {
    try {
      // eslint-disable-next-line no-await-in-loop
      const { stdout } = await execa('git', ['merge-base', 'HEAD', branch], { cwd })
      const commit = String(stdout).trim()
      if (commit !== '') return commit
    } catch {
      // Try the next candidate branch.
    }
  }
  return undefined
}

/**
 * The changesets-style bump picker: ask which packages get a major bump, then
 * which of the rest get a minor, and default whatever remains to patch. One
 * multiselect per level reads far better than a per-package prompt when many
 * packages are affected.
 */
async function promptBumpTypes (pkgRefs: string[]): Promise<Record<string, IntentBumpType>> {
  const bumpByRef = new Map<string, IntentBumpType>()
  let remaining = [...pkgRefs]
  for (const bumpType of ['major', 'minor'] as const) {
    if (remaining.length === 0) break
    // eslint-disable-next-line no-await-in-loop
    const chosen = new Set(await checkbox<string>({
      message: `Which packages should have a ${bumpType} bump?`,
      choices: remaining.map((ref) => ({ value: ref })),
    }))
    for (const ref of chosen) bumpByRef.set(ref, bumpType)
    remaining = remaining.filter((ref) => !chosen.has(ref))
  }
  for (const ref of remaining) bumpByRef.set(ref, 'patch')
  // Emit in the original selection order rather than grouped by bump level.
  return Object.fromEntries(pkgRefs.map((ref) => [ref, bumpByRef.get(ref)!]))
}

async function renderStatus (workspaceDir: string, opts: ChangeCommandOptions): Promise<string> {
  const intents = await readChangeIntents(workspaceDir)
  const ledger = await readLedger(workspaceDir)
  const baseArgs = {
    workspaceDir,
    projects: toWorkspaceProjects(opts.allProjects ?? []),
    intents,
    ledger,
    versioning: opts.versioning,
  }
  const unpublishedDirs = await resolveUnpublishedDirs(assembleReleasePlan(baseArgs), opts)
  const plan = assembleReleasePlan({ ...baseArgs, unpublishedDirs })
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

export interface ReleasableProject {
  name: string
  /** Workspace-relative project directory. */
  dir: string
  /**
   * How an intent file or versioning config should reference this project:
   * the bare name, or the `./`-prefixed directory when the name is shared by
   * several workspace projects.
   */
  ref: string
}

/**
 * The projects a change intent may demand a release from: named, carrying a
 * valid semver version, and not frozen by `versioning.ignore`. Matches the
 * participant set of the release-plan assembler.
 */
export function getReleasableProjects (
  allProjects: Array<Pick<Project, 'manifest' | 'rootDir'>>,
  workspaceDir: string,
  versioning?: VersioningSettings
): ReleasableProject[] {
  const refs = indexProjectRefs(allProjects, workspaceDir)
  const ignoredDirs = new Set((versioning?.ignore ?? []).flatMap((ref) => refs.refToDirs(ref)))
  return allProjects
    .filter(({ manifest }) =>
      manifest.name != null &&
      manifest.version != null &&
      valid(manifest.version) != null)
    .map((project) => ({ name: project.manifest.name!, dir: toProjectDir(workspaceDir, project.rootDir) }))
    .filter(({ dir }) => !ignoredDirs.has(dir))
    .map(({ name, dir }) => ({
      name,
      dir,
      ref: refs.nameToDirs(name).length > 1 ? `./${dir}` : name,
    }))
    .sort((left, right) => left.ref.localeCompare(right.ref))
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
