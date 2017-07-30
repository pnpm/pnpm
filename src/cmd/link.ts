import path = require('path')
import {
  link,
  linkFromGlobal,
  linkToGlobal,
  PnpmOptions
} from 'supi'

export default (
  input: string[],
  opts: PnpmOptions & {
    globalPrefix: string,
    globalBin: string,
  }
) => {
  const cwd = opts && opts.prefix || process.cwd()

  // pnpm link
  if (!input || !input.length) {
    return linkToGlobal(cwd, opts)
  }

  return input.reduce((previous: Promise<void>, inp: string) => {
    // pnpm link ../foo
    if (inp[0].indexOf('.') === 0) {
      const linkFrom = path.join(cwd, inp)
      return previous.then(() => link(linkFrom, cwd, opts))
    }

    // pnpm link foo
    return previous.then(() => linkFromGlobal(inp, cwd, opts))
  }, Promise.resolve())
}
