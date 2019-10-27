import path = require('path')

export default async function (
  args: string[],
  opts: {
    workingDir: string,
  },
  command: string,
) {
  return `${path.join(opts.workingDir, 'node_modules')}\n`
}
