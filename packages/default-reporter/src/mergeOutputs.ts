import { EOL } from './constants'
import * as Rx from 'rxjs'
import { filter, map, mergeAll, scan } from 'rxjs/operators'

export default function mergeOutputs (outputs: Array<Rx.Observable<Rx.Observable<{msg: string}>>>): Rx.Observable<string> {
  let blockNo = 0
  let fixedBlockNo = 0
  let started = false
  let previousOuput: string | null = null
  return Rx.merge(...outputs).pipe(
    map((log: Rx.Observable<{msg: string}>) => {
      let currentBlockNo = -1
      let currentFixedBlockNo = -1
      return log.pipe(
        map((msg) => {
          if (msg['fixed']) {
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
        delete acc.fixedBlocks[log['prevFixedBlockNo'] as number]
        acc.blocks[log.blockNo] = log.msg
      }
      return acc
    }, { fixedBlocks: [], blocks: [] } as {fixedBlocks: string[], blocks: string[]}),
    map((sections) => {
      const fixedBlocks = sections.fixedBlocks.filter(Boolean)
      const nonFixedPart = sections.blocks.filter(Boolean).join(EOL)
      if (!fixedBlocks.length) {
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
    filter((msg) => {
      if (msg !== previousOuput) {
        previousOuput = msg
        return true
      }
      return false
    })
  )
}
