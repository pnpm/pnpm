import { PnpmError } from '@pnpm/error'

import { readResponseBodyCapped } from '../tarball/readResponseBodyCapped.js'
import { MAX_TARBALL_BYTES } from '../tarball/summarizeTarball.js'
import type { StageContext } from './context.js'
import { stageRequest } from './request.js'

/** Download one staged package without writing it to disk. */
export async function fetchStageTarball (context: StageContext, stageId: string): Promise<Buffer> {
  const response = await stageRequest(context, {
    url: new URL(`-/stage/${stageId}/tarball`, context.registry).href,
    init: { method: 'GET' },
    action: `download staged package ${stageId}`,
  })
  const tarball = await readResponseBodyCapped(response, MAX_TARBALL_BYTES)
  if (tarball == null) {
    throw new PnpmError(
      'STAGE_REGISTRY_ERROR',
      `Failed to download staged package ${stageId}: registry response exceeded ${MAX_TARBALL_BYTES} bytes`
    )
  }
  return tarball
}
