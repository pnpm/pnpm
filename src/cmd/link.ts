import path = require('path')
import link, {linkFromGlobal, linkToGlobal} from '../api/link'
import {PnpmOptions} from '../types'

export default (input: string[], opts: PnpmOptions & {globalPrefix: string}) => {
  const cwd = opts && opts.prefix || process.cwd()

  // pnpm link
  if (!input || !input.length) {
    return linkToGlobal(cwd, opts)
  }

  return input.reduce((previous: Promise<any>, inp: string) => {
    // pnpm link ../foo
    if (inp[0].indexOf('.') === 0) {
      const linkFrom = path.join(cwd, inp)
      return previous.then(link.bind(null, linkFrom, cwd, opts))
    }

    // pnpm link foo
    return previous.then(linkFromGlobal.bind(null, inp, cwd, opts))
  }, Promise.resolve())
}
