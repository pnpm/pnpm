import type { StageLog, UnusedOverrideLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { buffer, filter, map } from 'rxjs/operators'

import { formatWarn } from './utils/formatWarn.js'

/**
 * Strip characters that can spoof or inject into terminal output from a
 * selector. Covers:
 *
 * - `Cc` (control): C0 (0x00–0x1F), DEL (0x7F), C1 (0x80–0x9F) — line
 *   moves, BEL, ESC/CSI sequences.
 * - `Cf` (format): zero-width and bidi overrides — U+200B–U+200F,
 *   U+2028–U+202E (line/paragraph separators + LRE/RLE/PDF/LRO/RLO,
 *   including U+202E RIGHT-TO-LEFT OVERRIDE), U+2060–U+2069 (invisible
 *   operators + bidi isolates), and U+FEFF (BOM / zero-width no-break
 *   space).
 *
 * The raw selector stays intact in the structured log event for machine
 * consumers.
 */
function sanitizeSelector (selector: string): string {
  // eslint-disable-next-line no-control-regex
  return selector.replace(/[\u0000-\u001F\u007F-\u009F\u200B-\u200F\u2028-\u202E\u2060-\u2069\uFEFF]/g, '')
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
    filter((unusedOverrides) => unusedOverrides.length > 0),
    map((unusedOverrides) => {
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
