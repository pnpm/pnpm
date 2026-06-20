import * as publishCommand from '../publish/publish.js'

export const STAGE_SUBCOMMANDS = ['publish', 'list', 'view', 'approve', 'reject', 'download'] as const
export type StageSubcommand = typeof STAGE_SUBCOMMANDS[number]

/**
 * Options accepted by every `pnpm stage` subcommand.
 *
 * `pnpm stage publish` forwards to {@link publishCommand.publish}, so we
 * intentionally inherit that command's option contract. The remaining
 * subcommands need only a subset (registry/auth/fetch/retry settings),
 * but accepting the full set keeps a single type across the dispatcher.
 */
export type StageOptions = Parameters<typeof publishCommand.publish>[0] & {
  cliOptions?: Record<string, unknown>
  json?: boolean
  otp?: string
  registry?: string
}

/**
 * Single staged package version as returned by the registry's `-/stage` and
 * `-/stage/<id>` endpoints. Every field is optional because the registry's
 * exact schema is not pinned, and the index signature keeps future fields
 * available to display without a code change.
 */
export interface StageItem {
  id?: string
  packageName?: string
  version?: string
  tag?: string
  createdAt?: string
  actor?: string
  actorType?: string
  shasum?: string
  [key: string]: unknown
}

export interface StageListResponse {
  items: StageItem[]
  total: number
}
