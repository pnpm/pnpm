import path = require('path')

export default async function (
  args: string[],
  opts: {
    dir: string,
  },
  command: string,
) {
  return `${path.join(opts.dir, 'node_modules')}\n`
}
