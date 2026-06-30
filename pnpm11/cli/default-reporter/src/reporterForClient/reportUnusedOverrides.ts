import type { StageLog, UnusedOverrideLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { buffer, filter, map } from 'rxjs/operators'

import { formatWarn } from './utils/formatWarn.js'

/**
 * Strip ASCII control characters (C0 range 0x00–0x1F and DEL 0x7F)
 * from a selector so a crafted override key containing `\n`, `\r`, or
 * ESC cannot inject/spoof terminal output. The raw selector stays
 * intact in the structured log event for machine consumers.
 */
function sanitizeSelector (selector: string): string {
  // eslint-disable-next-line no-control-regex
  return selector.replace(/[\x00-\x1F\x7F]/g, '')
}

export function reportUnusedOverrides (
  log$: {
    unusedOverride: Rx.Observable<UnusedOverrideLog>
    stage: Rx.Observable<StageLog>
  }
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  const resolutionDone$ = log$.stage.pipe(
    filter((log) => log.stage === 'resolution_done')
  )
  return log$.unusedOverride.pipe(
    buffer(resolutionDone$),
    map((unusedOverrides) => {
      if (unusedOverrides.length === 0) return Rx.EMPTY
      const selectors = unusedOverrides
        .map((log) => sanitizeSelector(log.selector))
        .sort()
      const head = selectors.length === 1
        ? '1 override matched no dependency'
        : `${selectors.length} overrides matched no dependency`
      return Rx.of({
        msg: formatWarn(`${head}: ${selectors.join(', ')}`),
      })
    })
  )
}
