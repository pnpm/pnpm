import path = require('path')
import list, {forPackages as listForPackages} from 'pnpm-list'

const LAYOUT_VERSION = '1'

export default async function (
  args: string[],
  opts: {
    prefix: string,
    depth?: number,
    only?: 'dev' | 'prod',
    long?: boolean,
    parseable?: boolean,
    global: boolean,
    independentLeaves: boolean,
    alwaysPrintRootPackage?: boolean,
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

  opts.long = opts.long || command === 'll' || command === 'la'
  const output = args.length
    ? await listForPackages(args, prefix, opts)
    : await list(prefix, opts)

  if (output) console.log(output)
}
