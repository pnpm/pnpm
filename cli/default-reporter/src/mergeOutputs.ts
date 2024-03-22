import * as Rx from 'rxjs'
import { filter, map, mergeAll, scan } from 'rxjs/operators'

import { EOL } from './constants.js'

export function mergeOutputs(
  outputs: Array<Rx.Observable<Rx.Observable<{ msg: string }>>>
): Rx.Observable<string> {
  let blockNo = 0

  let fixedBlockNo = 0

  let started = false

  let previousOutput: string | null = null

  return Rx.merge(...outputs).pipe(
    map((log: Rx.Observable<{ msg: string, fixed?: boolean | undefined }>): Rx.Observable<{
      blockNo: number;
      fixed: boolean;
      msg: string;
      prevFixedBlockNo?: number | undefined;
    } | {
      blockNo: number;
      fixed: boolean;
      msg: string;
      prevFixedBlockNo: number;
    }> => {
      let currentBlockNo = -1

      let currentFixedBlockNo = -1

      return log.pipe(
        map((msg: {
          msg: string;
          fixed?: boolean | undefined;
        }) => {
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
    scan(
      (acc: {
        fixedBlocks: string[]
        blocks: string[]
      }, log) => {
        if (log.fixed) {
          acc.fixedBlocks[log.blockNo] = log.msg
        } else {
          if (typeof log.prevFixedBlockNo !== 'undefined') {
            delete acc.fixedBlocks[log.prevFixedBlockNo]
            acc.blocks[log.blockNo] = log.msg
          }
        }

        return acc
      },
      { fixedBlocks: [], blocks: [] }
    ),
    map((sections: {
      fixedBlocks: string[];
      blocks: string[];
    }): string => {
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
    filter((msg: string): boolean => {
      if (started) {
        return true
      }

      if (msg === '') {
        return false
      }

      started = true

      return true
    }),
    filter((msg: string): boolean => {
      if (msg !== previousOutput) {
        previousOutput = msg

        return true
      }

      return false
    })
  )
}
