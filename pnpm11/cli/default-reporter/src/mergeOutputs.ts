import * as Rx from 'rxjs'
import { filter, map, mergeAll, scan } from 'rxjs/operators'

import { EOL } from './constants.js'

interface OutputMessage {
  msg: string
  fixed?: boolean
}

interface BlockMessage {
  blockNo: number
  fixed: boolean
  msg: string
  prevFixedBlockNo?: number
}

interface Sections {
  fixedBlocks: string[]
  blocks: string[]
}

export function mergeOutputs (outputs: Array<Rx.Observable<Rx.Observable<{ msg: string }>>>): Rx.Observable<string> {
  const blockCounters = { blockNo: 0, fixedBlockNo: 0 }
  let started = false
  let previousOutput: string | null = null
  return Rx.merge(...outputs).pipe(
    map((log: Rx.Observable<OutputMessage>) => log.pipe(map(numberBlocks(blockCounters)))),
    mergeAll(),
    scan(placeBlock, { fixedBlocks: [], blocks: [] } as Sections),
    map(joinSections),
    filter((msg) => {
      if (started) {
        return true
      }
      if (msg === '') return false
      started = true
      return true
    }),
    filter((msg) => {
      if (msg !== previousOutput) {
        previousOutput = msg
        return true
      }
      return false
    })
  )
}

/**
 * Assigns the messages of one output to a block. An output keeps the block it
 * got on its first message; fixed messages get a fixed block of their own.
 */
function numberBlocks (blockCounters: { blockNo: number, fixedBlockNo: number }): (msg: OutputMessage) => BlockMessage {
  let currentBlockNo = -1
  let currentFixedBlockNo = -1
  return (msg) => {
    if (msg.fixed) {
      if (currentFixedBlockNo === -1) {
        currentFixedBlockNo = blockCounters.fixedBlockNo++
      }
      return {
        blockNo: currentFixedBlockNo,
        fixed: true,
        msg: msg.msg,
      }
    }
    if (currentBlockNo === -1) {
      currentBlockNo = blockCounters.blockNo++
    }
    return {
      blockNo: currentBlockNo,
      fixed: false,
      msg: typeof msg === 'string' ? msg : msg.msg, // eslint-disable-line
      prevFixedBlockNo: currentFixedBlockNo,
    }
  }
}

function placeBlock (acc: Sections, log: BlockMessage): Sections {
  if (log.fixed) {
    acc.fixedBlocks[log.blockNo] = log.msg
  } else {
    delete acc.fixedBlocks[log.prevFixedBlockNo as number]
    acc.blocks[log.blockNo] = log.msg
  }
  return acc
}

function joinSections (sections: Sections): string {
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
}
