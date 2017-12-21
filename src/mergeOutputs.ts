import chalk from 'chalk'
import os = require('os')
import xs, {Stream} from 'xstream'
import dropRepeats from 'xstream/extra/dropRepeats'
import flattenConcurrently from 'xstream/extra/flattenConcurrently'

const EOL = os.EOL

export default function mergeOutputs (outputs: Array<xs<xs<{msg: string}>>>): Stream<string> {
  let blockNo = 0
  let fixedBlockNo = 0
  let started = false
  return flattenConcurrently(
    (xs.merge.apply(xs, outputs) as xs<xs<{msg: string}>>)
    .map((log: Stream<{msg: string, fixed: boolean}>) => {
      let currentBlockNo = -1
      let currentFixedBlockNo = -1
      let calculated = false
      let fixedCalculated = false
      return log
        .map((msg) => {
          if (msg['fixed']) {
            if (!fixedCalculated) {
              fixedCalculated = true
              currentFixedBlockNo = fixedBlockNo++
            }
            return {
              blockNo: currentFixedBlockNo,
              fixed: true,
              msg: msg.msg,
            }
          }
          if (!calculated) {
            calculated = true
            currentBlockNo = blockNo++
          }
          return {
            blockNo: currentBlockNo,
            fixed: false,
            msg: typeof msg === 'string' ? msg : msg.msg,
            prevFixedBlockNo: currentFixedBlockNo,
          }
        })
    }),
  )
  .fold((acc, log) => {
    if (log.fixed === true) {
      acc.fixedBlocks[log.blockNo] = log.msg
    } else {
      delete acc.fixedBlocks[log['prevFixedBlockNo']]
      acc.blocks[log.blockNo] = log.msg
    }
    return acc
  }, {fixedBlocks: [], blocks: []} as {fixedBlocks: string[], blocks: string[]})
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
    return chalk.dim(nonFixedPart) + EOL + fixedPart
  })
  .filter((msg) => {
    if (started) {
      return true
    }
    if (msg === '') return false
    started = true
    return true
  })
  .compose(dropRepeats())
}
