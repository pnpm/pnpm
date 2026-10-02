import { redactUrlForDisplay } from '@pnpm/error'
import semver from 'semver'

import { applyWorkflowUpdates } from './applyWorkflowEdits.js'
import { createUpdatePlan } from './createUpdatePlan.js'
import { resolveServerUrl } from './resolveServerUrl.js'
import type {
  FindOutdatedGitHubActionsOptions,
  GitHubActionsOptInOptions,
  OutdatedGitHubAction,
  PlannedUpdate,
  RepoVersion,
  UpdateGitHubActionsOptions,
} from './types.js'

export type {
  FindOutdatedGitHubActionsOptions,
  GitHubActionsOptInOptions,
  GitHubActionsOptions,
  OutdatedGitHubAction,
  UpdateGitHubActionsOptions,
} from './types.js'

/**
 * GitHub Actions dependencies are opt-in. Reading them means running
 * `git ls-remote` against every referenced repository, so `pnpm outdated` and
 * `pnpm update` only look at workflow files when asked to, either with
 * `--include-github-actions` or with `update.githubActions: true`.
 */
export function shouldCheckGitHubActions (opts: GitHubActionsOptInOptions): boolean {
  return opts.includeGithubActions === true || opts.updateConfig?.githubActions === true
}

export function isGitHubActionSelector (selector: string): boolean {
  const pattern = selector.startsWith('!') ? selector.slice(1) : selector
  return !pattern.startsWith('@') && pattern.includes('/')
}

export function normalizeGitHubActionSelector (selector: string): string {
  if (!isGitHubActionSelector(selector)) return selector
  const refSeparator = selector.lastIndexOf('@')
  return refSeparator === -1 ? selector : selector.slice(0, refSeparator)
}

export async function findOutdatedGitHubActions (
  opts: FindOutdatedGitHubActionsOptions
): Promise<OutdatedGitHubAction[]> {
  const plans = await createUpdatePlan(opts)
  const serverUrl = resolveServerUrl(opts.serverUrl)
  const target = (plan: PlannedUpdate) => opts.compatible ? plan.wanted : plan.latest
  return dedupeOutdated(plans
    .filter((plan) => semver.lt(plan.current.version, target(plan).version))
    .map((plan) => toOutdatedGitHubAction(plan, target(plan), serverUrl)))
}

export async function updateGitHubActions (
  opts: UpdateGitHubActionsOptions
): Promise<OutdatedGitHubAction[]> {
  const plans = await createUpdatePlan(opts)
  const target = (plan: PlannedUpdate) => opts.latest ? plan.latest : plan.wanted
  const updates = plans.filter((plan) => needsUpdate(plan, target(plan)))
  await applyWorkflowUpdates(updates, target)
  const serverUrl = resolveServerUrl(opts.serverUrl)
  return dedupeOutdated(updates.map((plan) => toOutdatedGitHubAction(plan, target(plan), serverUrl)))
}

function needsUpdate (plan: PlannedUpdate, target: RepoVersion): boolean {
  return semver.lte(plan.current.version, target.version) &&
    (plan.action.ref !== target.commit || plan.action.commentVersion !== target.tag)
}

function toOutdatedGitHubAction (plan: PlannedUpdate, target: RepoVersion, serverUrl: string): OutdatedGitHubAction {
  return {
    current: plan.current.version.version,
    latest: target.version.version,
    name: plan.action.name,
    wanted: plan.wanted.version.version,
    homepage: redactUrlForDisplay(`${serverUrl}/${plan.action.repo}`),
  }
}

function dedupeOutdated (actions: OutdatedGitHubAction[]): OutdatedGitHubAction[] {
  return [...new Map(actions.map((action) => [action.name, action])).values()]
    .sort((left, right) => left.name.localeCompare(right.name))
}
