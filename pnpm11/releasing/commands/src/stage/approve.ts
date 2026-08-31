import util from 'node:util'

import { checkbox } from '@inquirer/prompts'
import { PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { sanitizeInline } from '@pnpm/text.sanitize'
import chalk from 'chalk'
import validateNpmPackageName from 'validate-npm-package-name'

import {
  readWorkspaceApprovalOrder,
  sortStageItemsForApproval,
  unavailableDependencies,
} from './approvalOrder.js'
import { createStageContext, type StageContext } from './context.js'
import type { StageRegistryError } from './errors.js'
import { fetchStageItems } from './items.js'
import { parseStageIds, UUID_REGEX } from './parsing.js'
import { createStageOtpSession, stageJsonRequest, type StageOtpSession } from './request.js'
import type { ApprovalItem, StageItem, StageOptions } from './types.js'

export type StageApproveResult = string | { output: string, exitCode: number }

/**
 * `pnpm stage approve [<stage-id> ...]` — publish staged versions, choosing
 * them interactively when none are named.
 *
 * Several versions are approved through a single {@link StageOtpSession}, so
 * one proof of presence covers the whole batch, and in workspace dependency
 * order, so a package reaches the registry only after the workspace packages
 * it depends on.
 */
export async function stageApprove (opts: StageOptions, params: string[]): Promise<StageApproveResult> {
  const context = createStageContext(opts)
  if (params.length === 0) {
    requireInteractiveSelection()
    const stagedPackages = (await fetchStageItems(context)).map(toApprovalItem).filter((item) => item != null)
    if (stagedPackages.length === 0) return 'There are no staged packages awaiting approval.'
    const selected = await promptForStagedPackages(stagedPackages)
    if (selected.length === 0) return 'No staged packages were selected.'
    return approveStagedPackages(context, opts, selected)
  }
  const stageIds = dedupeStageIds(parseStageIds(params, 'approve'))
  if (stageIds.length === 1) {
    const [stageId] = stageIds
    await approveStagedPackage(context, { id: stageId }, createStageOtpSession(context))
    return `Staged package ${stageId} approved and published successfully.`
  }
  return approveStagedPackages(context, opts, await resolveStageItems(context, stageIds))
}

/**
 * A staged version repeated on the command line is one approval: sending the
 * second request would either fail against the release the first one
 * published, or count the same package twice. Stage ids are hexadecimal, so
 * the same id in two spellings is the same id; the first spelling is the one
 * that reaches the registry.
 */
function dedupeStageIds (stageIds: string[]): string[] {
  const seen = new Set<string>()
  return stageIds.filter((stageId) => {
    const key = stageId.toLowerCase()
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

/**
 * Reads one staged version the registry reported.
 *
 * The entry is registry-controlled input that ends up in a terminal prompt the
 * user picks releases from, so every field is taken as it came and checked
 * rather than repaired:
 *
 * - the id has to be the same UUID the other subcommands address a staged
 *   version by, and the package name a name npm would accept — both checked
 *   before anything is stripped, so that removing a hidden character can
 *   never be what makes a value valid. A name that fails carries no workspace
 *   identity, so the version is approved outside the workspace order rather
 *   than under the name it resembles;
 * - the remaining fields are shown and nothing more, so they are stripped of
 *   the control characters that could redraw the prompt around a selection.
 */
function toApprovalItem (item: StageItem): ApprovalItem | undefined {
  if (typeof item.id !== 'string' || !UUID_REGEX.test(item.id)) return undefined
  return {
    id: item.id,
    packageName: validPackageName(item.packageName),
    version: sanitizedField(item.version),
    tag: sanitizedField(item.tag),
    createdAt: sanitizedField(item.createdAt),
    actor: sanitizedField(item.actor),
  }
}

/**
 * The name a staged version publishes under, if the registry named it the way
 * npm would. A valid name is URL-safe, so it is also safe to display as it is.
 */
function validPackageName (value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  return validateNpmPackageName(value).validForOldPackages ? value : undefined
}

function sanitizedField (value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  const sanitized = sanitizeInline(value)
  return sanitized === '' ? undefined : sanitized
}

function requireInteractiveSelection (): void {
  if (process.stdin.isTTY && process.stdout.isTTY) return
  throw new PnpmError('STAGE_ID_REQUIRED', 'Missing required <stage-id> for "pnpm stage approve"', {
    hint: 'Run "pnpm stage approve" in an interactive terminal to choose from the staged packages, or pass the stage ids to approve.',
  })
}

/** Shows the checkbox prompt; an interrupted prompt selects nothing. */
async function promptForStagedPackages (stagedPackages: ApprovalItem[]): Promise<ApprovalItem[]> {
  try {
    return await checkbox({
      choices: stagedPackages.map((item) => ({
        name: renderStageItemChoice(item),
        value: item,
      })),
      message: 'Choose which staged packages to approve ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
      required: false,
      theme: {
        icon: { checked: '●', unchecked: '○', cursor: '❯' },
        style: {
          highlight: chalk.bgBlack.whiteBright,
        },
        keybindings: ['vim'],
      },
    })
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && err.name === 'ExitPromptError') return []
    throw err
  }
}

/**
 * The staged versions the given ids identify, each read from the registry's
 * entry for that id rather than from the full staged listing, which a busy
 * registry can page far beyond what the batch needs.
 *
 * A version the registry does not describe is kept as its bare id, so
 * approving it fails on the registry's own error rather than on a guess about
 * why it is missing; it also carries no package name, so it is approved
 * outside the workspace order.
 */
async function resolveStageItems (context: StageContext, stageIds: string[]): Promise<ApprovalItem[]> {
  return Promise.all(stageIds.map(async (stageId) => {
    let item: StageItem
    try {
      item = await stageJsonRequest<StageItem>(context, {
        url: new URL(`-/stage/${stageId}`, context.registry).href,
        action: `view staged package ${stageId}`,
      })
    } catch (err: unknown) {
      // Only the registry answering "no such staged version" is survivable
      // here. An authentication failure or a broken connection applies to
      // every id in the batch, so it aborts before anything is approved.
      if (!isMissingStageError(err)) throw err
      return { id: stageId }
    }
    return toApprovalItem({ ...item, id: stageId }) ?? { id: stageId }
  }))
}

async function approveStagedPackages (
  context: StageContext,
  opts: StageOptions,
  items: ApprovalItem[]
): Promise<StageApproveResult> {
  const order = await readWorkspaceApprovalOrder(opts)
  const sortedItems = sortStageItemsForApproval(items, order)
  const session = createStageOtpSession(context)
  const unpublishedPackageNames = new Set<string>()
  let approvedCount = 0
  for (const item of sortedItems) {
    const label = renderStageItemLabel(item)
    const blockers = unavailableDependencies(item, unpublishedPackageNames, order)
    if (blockers.length > 0) {
      if (item.packageName) unpublishedPackageNames.add(item.packageName)
      globalWarn(`Skipped ${label}, as it depends on ${blockers.join(', ')}, which could not be approved`)
      continue
    }
    try {
      // eslint-disable-next-line no-await-in-loop
      await approveStagedPackage(context, item, session)
      approvedCount++
      globalInfo(`Approved ${label}`)
    } catch (err: unknown) {
      // Only the registry's verdict on one staged version is survivable. An
      // authentication failure or a broken connection applies to every
      // remaining version too, so it aborts the batch.
      if (!isStageRegistryError(err)) throw err
      if (item.packageName) unpublishedPackageNames.add(item.packageName)
      globalWarn(err.message)
    }
  }
  const failedCount = sortedItems.length - approvedCount
  return {
    output: failedCount === 0
      ? `Approved ${renderPackageCount(approvedCount)} successfully.`
      : `Approved ${approvedCount} of ${renderPackageCount(sortedItems.length)}.`,
    exitCode: failedCount === 0 ? 0 : 1,
  }
}

async function approveStagedPackage (context: StageContext, item: ApprovalItem, session: StageOtpSession): Promise<void> {
  await session.request({
    url: new URL(`-/stage/${item.id}/approve`, context.registry).href,
    init: { method: 'POST' },
    action: `approve staged package ${renderStageItemReference(item)}`,
  })
}

function isStageRegistryError (err: unknown): err is Error {
  return util.types.isNativeError(err) && (err as PnpmError).code === 'ERR_PNPM_STAGE_REGISTRY_ERROR'
}

function isMissingStageError (err: unknown): boolean {
  return isStageRegistryError(err) && (err as StageRegistryError).status === 404
}

/** How the interactive picker names a staged version. */
function renderStageItemChoice (item: ApprovalItem): string {
  const details = [item.tag, item.createdAt && `staged ${item.createdAt}`, item.actor && `by ${item.actor}`]
    .filter((detail): detail is string => detail != null && detail !== '')
  return details.length > 0
    ? `${renderStageItemLabel(item)} (${details.join(', ')})`
    : renderStageItemLabel(item)
}

function renderStageItemLabel (item: ApprovalItem): string {
  if (!item.packageName) return item.id
  return item.version ? `${item.packageName}@${item.version}` : item.packageName
}

/** The label an error message identifies a staged version by. */
function renderStageItemReference (item: ApprovalItem): string {
  return item.packageName ? `${renderStageItemLabel(item)} (${item.id})` : item.id
}

function renderPackageCount (count: number): string {
  return `${count} staged package${count === 1 ? '' : 's'}`
}
