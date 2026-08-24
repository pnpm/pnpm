import util from 'node:util'

import { checkbox } from '@inquirer/prompts'
import { PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { sanitizeInline } from '@pnpm/text.sanitize'
import chalk from 'chalk'

import {
  readWorkspaceApprovalOrder,
  sortStageItemsForApproval,
  unavailableDependencies,
} from './approvalOrder.js'
import { createStageContext, type StageContext } from './context.js'
import { fetchStageItems } from './items.js'
import { parseStageIds, UUID_REGEX } from './parsing.js'
import { createStageOtpSession, type StageOtpSession } from './request.js'
import type { StageItem, StageOptions } from './types.js'

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
    const stagedPackages = (await fetchStageItems(context)).filter(hasStageId)
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
 * A staged version the registry reported can only be approved through the id
 * the other subcommands address it by — the same UUID the command line
 * accepts, not an arbitrary string the registry puts in the listing.
 */
function hasStageId (item: StageItem): item is StageItem & { id: string } {
  return typeof item.id === 'string' && UUID_REGEX.test(item.id)
}

function requireInteractiveSelection (): void {
  if (process.stdin.isTTY && process.stdout.isTTY) return
  throw new PnpmError('STAGE_ID_REQUIRED', 'Missing required <stage-id> for "pnpm stage approve"', {
    hint: 'Run "pnpm stage approve" in an interactive terminal to choose from the staged packages, or pass the stage ids to approve.',
  })
}

/** Shows the checkbox prompt; an interrupted prompt selects nothing. */
async function promptForStagedPackages (stagedPackages: StageItem[]): Promise<StageItem[]> {
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
 * The staged versions the given ids identify. Stage ids are hexadecimal, so
 * the lookup ignores their spelling: an id the listing carries in another
 * casing still resolves to the package name the approval order and the
 * dependency blocking are keyed on. An id the registry does not list at all is
 * kept as is, so approving it fails on the registry's own error rather than on
 * a guess about why it is missing.
 */
async function resolveStageItems (context: StageContext, stageIds: string[]): Promise<StageItem[]> {
  const listedItems = (await fetchStageItems(context)).filter(hasStageId)
  const itemsByStageId = new Map(listedItems.map((item) => [item.id.toLowerCase(), item]))
  return stageIds.map((stageId) => itemsByStageId.get(stageId.toLowerCase()) ?? { id: stageId })
}

async function approveStagedPackages (
  context: StageContext,
  opts: StageOptions,
  items: StageItem[]
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

async function approveStagedPackage (context: StageContext, item: StageItem, session: StageOtpSession): Promise<void> {
  await session.request({
    url: new URL(`-/stage/${item.id}/approve`, context.registry).href,
    init: { method: 'POST' },
    action: `approve staged package ${renderStageItemReference(item)}`,
  })
}

function isStageRegistryError (err: unknown): err is Error {
  return util.types.isNativeError(err) && (err as PnpmError).code === 'ERR_PNPM_STAGE_REGISTRY_ERROR'
}

/**
 * How the interactive picker names a staged version.
 *
 * The fields come from the registry and end up in a prompt the user picks
 * releases from, so {@link renderRegistryText} keeps them from redrawing it.
 */
function renderStageItemChoice (item: StageItem): string {
  const details = [item.tag, item.createdAt && `staged ${item.createdAt}`, item.actor && `by ${item.actor}`]
    .filter((detail): detail is string => detail != null && detail !== '')
    .map((detail) => renderRegistryText(detail))
  return details.length > 0
    ? `${renderStageItemLabel(item)} (${details.join(', ')})`
    : renderStageItemLabel(item)
}

function renderStageItemLabel (item: StageItem): string {
  if (!item.packageName) return item.id ?? '<unknown staged package>'
  const packageName = renderRegistryText(item.packageName)
  return item.version ? `${packageName}@${renderRegistryText(item.version)}` : packageName
}

/** Registry-provided text, stripped of what could rewrite the terminal. */
function renderRegistryText (text: string): string {
  return sanitizeInline(text)
}

/** The label an error message identifies a staged version by. */
function renderStageItemReference (item: StageItem): string {
  return item.packageName ? `${renderStageItemLabel(item)} (${item.id})` : item.id ?? '<unknown staged package>'
}

function renderPackageCount (count: number): string {
  return `${count} staged package${count === 1 ? '' : 's'}`
}
