import { createStageContext } from './context.js'
import { requireStageId } from './parsing.js'
import { stageRequestWithOtp } from './request.js'
import type { StageOptions } from './types.js'

export async function stageApprove (opts: StageOptions, params: string[]): Promise<string> {
  const stageId = requireStageId(params, 'approve')
  const context = createStageContext(opts)
  await stageRequestWithOtp(context, {
    url: new URL(`-/stage/${stageId}/approve`, context.registry).href,
    init: { method: 'POST' },
    action: `approve staged package ${stageId}`,
  })
  return `Staged package ${stageId} approved and published successfully.`
}
