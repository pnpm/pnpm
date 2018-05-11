import path = require('path')

const LAYOUT_VERSION = '1'

export default async function (
  args: string[],
  opts: {
    prefix: string,
    global: boolean,
    independentLeaves: boolean,
  },
  command: string,
) {
  let prefix: string
  if (opts.global) {
    prefix = path.join(opts.prefix, LAYOUT_VERSION)
    if (opts.independentLeaves) {
      prefix += '_independent_leaves'
    }
  } else {
    prefix = opts.prefix
  }

  console.log(path.join(prefix, 'node_modules'))
}
