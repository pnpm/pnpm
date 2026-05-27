import { globalWarn } from '@pnpm/logger'

import { createStageContext } from './context.js'
import { requireStageId } from './parsing.js'
import { stageRequestWithOtp } from './request.js'
import type { StageOptions } from './types.js'

export async function stageReject (opts: StageOptions, params: string[]): Promise<string> {
  const stageId = requireStageId(params, 'reject')
  const context = createStageContext(opts)
  globalWarn('Rejecting will permanently delete this staged publish record and tarball from the registry.')
  await stageRequestWithOtp(context, {
    url: new URL(`-/stage/${stageId}`, context.registry).href,
    init: { method: 'DELETE' },
    action: `reject staged package ${stageId}`,
  })
  return `Staged package ${stageId} has been rejected.`
}
