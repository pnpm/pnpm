import logUpdate = require('log-update')

let fixed: string | null

export function write (line: string) {
  logUpdate(line)
  logUpdate.done()
  if (fixed) logUpdate(fixed)
}

export function fixedWrite (line: string) {
  fixed = line
  logUpdate(line)
}

export function done () {
  fixed = null
  logUpdate.done()
}
