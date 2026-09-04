import type { StageContext } from './context.js'
import { stageRequest } from './request.js'

/** Download one staged package without writing it to disk. */
export async function fetchStageTarball (context: StageContext, stageId: string): Promise<Buffer> {
  const response = await stageRequest(context, {
    url: new URL(`-/stage/${stageId}/tarball`, context.registry).href,
    init: { method: 'GET' },
    action: `download staged package ${stageId}`,
  })
  return Buffer.from(await response.arrayBuffer())
}
