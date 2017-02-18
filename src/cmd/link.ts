import path = require('path')
import link, {linkFromGlobal, linkToGlobal} from '../api/link'
import {PnpmOptions} from '../types'

export default (input: string[], opts: PnpmOptions) => {
  const cwd = opts && opts.cwd || process.cwd()

  // pnpm link
  if (!input || !input.length) {
    return linkToGlobal(cwd, opts)
  }

  // pnpm link ../foo
  if (input[0].indexOf('.') === 0) {
    const linkFrom = path.join(cwd, input[0])
    return link(linkFrom, cwd, opts)
  }

  // pnpm link foo
  return linkFromGlobal(input[0], cwd, opts)
}
