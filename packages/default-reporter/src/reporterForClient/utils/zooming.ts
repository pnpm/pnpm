import { PREFIX_MAX_LENGTH } from '../outputConstants'
import formatPrefix from './formatPrefix'
import rightPad = require('right-pad')

export function autozoom (
  currentPrefix: string,
  logPrefix: string | undefined,
  line: string,
  opts: {
    zoomOutCurrent: boolean
  }
) {
  if (!logPrefix || !opts.zoomOutCurrent && currentPrefix === logPrefix) {
    return line
  }
  return zoomOut(currentPrefix, logPrefix, line)
}

export function zoomOut (currentPrefix: string, logPrefix: string, line: string) {
  return `${rightPad(formatPrefix(currentPrefix, logPrefix), PREFIX_MAX_LENGTH)} | ${line}`
}
