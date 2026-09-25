import { createStageContext } from './context.js'
import { requireStageId } from './parsing.js'
import { renderStageItem } from './rendering.js'
import { stageJsonRequest } from './request.js'
import type { StageItem, StageOptions } from './types.js'

export async function stageView (opts: StageOptions, params: string[]): Promise<string> {
  const stageId = requireStageId(params, 'view')
  const context = createStageContext(opts)
  const item = await stageJsonRequest<StageItem>(context, {
    url: new URL(`-/stage/${stageId}`, context.registry).href,
    action: `view staged package ${stageId}`,
  })
  return opts.json ? JSON.stringify(item, null, 2) : renderStageItem(item)
}
