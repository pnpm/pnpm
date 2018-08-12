import path = require('path')

const LAYOUT_VERSION = '1'

export default async function (
  args: string[],
  opts: {
    prefix: string,
  },
  command: string,
) {
  console.log(path.join(opts.prefix, 'node_modules'))
}
