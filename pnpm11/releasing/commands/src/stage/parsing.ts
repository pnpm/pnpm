import { PnpmError } from '@pnpm/error'
import npa from '@pnpm/npm-package-arg'

import type { StageSubcommand } from './types.js'

export const UUID_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

export function parseStagePackageSpec (rawSpec: string): { name: string, rawSpec: string } {
  let spec: ReturnType<typeof npa>
  try {
    spec = npa(rawSpec)
  } catch {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${rawSpec}`)
  }
  if (!spec.name) {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${rawSpec}`)
  }
  return { name: spec.name, rawSpec: spec.rawSpec }
}

/**
 * Validates every stage id in `params`. An empty `params` yields an empty
 * array — the caller decides whether that is an error or a request for
 * interactive selection.
 */
export function parseStageIds (params: string[], subcommand: StageSubcommand): string[] {
  return params.map((stageId) => requireStageId([stageId], subcommand))
}

export function requireStageId (params: string[], subcommand: StageSubcommand): string {
  if (!params[0]) {
    throw new PnpmError('STAGE_ID_REQUIRED', `Missing required <stage-id> for "pnpm stage ${subcommand}"`)
  }
  const stageId = params[0]
  if (!UUID_REGEX.test(stageId)) {
    throw new PnpmError('INVALID_STAGE_ID', 'stage-id must be a valid UUID')
  }
  return stageId
}
