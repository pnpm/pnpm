import { EOL } from './constants'
import most = require('most')

export default function mergeOutputs (outputs: Array<most.Stream<most.Stream<{msg: string}>>>): most.Stream<string> {
  let blockNo = 0
  let fixedBlockNo = 0
  let started = false
  return most.join(
    most.mergeArray(outputs)
      .map((log: most.Stream<{msg: string}>) => {
        let currentBlockNo = -1
        let currentFixedBlockNo = -1
        return log
          .map((msg) => {
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
      })
  )
    .scan((acc, log) => {
      if (log.fixed === true) {
        acc.fixedBlocks[log.blockNo] = log.msg
      } else {
        delete acc.fixedBlocks[log['prevFixedBlockNo'] as number]
        acc.blocks[log.blockNo] = log.msg
      }
      return acc
    }, { fixedBlocks: [], blocks: [] } as {fixedBlocks: string[], blocks: string[]})
    .map((sections) => {
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
    })
    .filter((msg) => {
      if (started) {
        return true
      }
      if (msg === '') return false
      started = true
      return true
    })
    .skipRepeats()
}
