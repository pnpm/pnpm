// From https://www.npmjs.com/package/ndjson, but with updated deps, only parse and hardcoded options

import type { Transform } from 'stream'

import split from 'split2'

const opts = { strict: true }

export function parse (): Transform {
  function parseRow (this: Transform, row: string) {
    try {
      if (row) return JSON.parse(row)
    } catch (e) {
      if (opts.strict) {
        this.emit('error', new Error(`Could not parse row "${row.length > 50 ? `${row.slice(0, 50)}...` : row}"`))
      }
    }
  }

  return split(parseRow, opts)
}
