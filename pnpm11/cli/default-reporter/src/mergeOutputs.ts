import * as Rx from 'rxjs'
import { filter, map, mergeAll, scan } from 'rxjs/operators'

import { EOL } from './constants.js'

export function mergeOutputs (outputs: Array<Rx.Observable<Rx.Observable<{ msg: string }>>>): Rx.Observable<string> {
  let blockNo = 0
  let fixedBlockNo = 0
  let started = false
  let previousOutput: string | null = null
  return Rx.merge(...outputs).pipe(
    map((log: Rx.Observable<{ msg: string, fixed?: boolean }>) => {
      let currentBlockNo = -1
      let currentFixedBlockNo = -1
      return log.pipe(
        map((msg) => {
          if (msg.fixed) {
            if (currentFixedBlockNo === -1) {
              currentFixedBlockNo = fixedBlockNo++
            }
            return {
              blockNo: currentFixedBlockNo,
              fixed: true,
              msg: msg.msg,
            }
          }
          if (currentBlockNo === -1) {
            currentBlockNo = blockNo++
          }
          return {
            blockNo: currentBlockNo,
            fixed: false,
          msg: typeof msg === 'string' ? msg : msg.msg, // eslint-disable-line
            prevFixedBlockNo: currentFixedBlockNo,
          }
        })
      )
    }),
    mergeAll(),
    scan((acc, log) => {
      if (log.fixed) {
        acc.fixedBlocks[log.blockNo] = log.msg
      } else {
        delete acc.fixedBlocks[log.prevFixedBlockNo as number]
        acc.blocks[log.blockNo] = log.msg
      }
      return acc
    }, { fixedBlocks: [], blocks: [] } as { fixedBlocks: string[], blocks: string[] }),
    map((sections) => {
      const fixedBlocks = sections.fixedBlocks.filter(Boolean)
      const nonFixedPart = sections.blocks.filter(Boolean).join(EOL)
      if (fixedBlocks.length === 0) {
        return nonFixedPart
      }
      const fixedPart = fixedBlocks.join(EOL)
      if (!nonFixedPart) {
        return fixedPart
      }
      return `${nonFixedPart}${EOL}${fixedPart}`
    }),
    filter((msg) => {
      if (started) {
        return true
      }
      if (msg === '') return false
      started = true
      return true
    }),
    // An empty combined string means every block has been cleared — most
    // commonly from the cached-then-clear pair `reportLockfileVerification`
    // emits to drive the fixed-block deletion through `scan` above. The
    // state update has already happened by this point; the empty frame
    // itself carries no information for the renderer and must not reach
    // `logUpdate`, which appends EOL unconditionally and would write a
    // visible blank line in captured TTY output (`script`, CI TTY
    // captures).
    filter((msg) => msg !== ''),
    filter((msg) => {
      if (msg !== previousOutput) {
        previousOutput = msg
        return true
      }
      return false
    })
  )
}
