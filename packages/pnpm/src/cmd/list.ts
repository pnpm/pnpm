import path = require('path')
import list, {forPackages as listForPackages} from 'pnpm-list'

const LAYOUT_VERSION = '1'

export default async function (
  args: string[],
  opts: {
    prefix: string,
    depth?: number,
    long?: boolean,
    parseable?: boolean,
    production: boolean,
    development: boolean,
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
  const only = (opts.production && opts.development ? undefined : (opts.production ? 'prod' : 'dev')) as ('prod' | 'dev' | undefined)
  const listOpts = {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: opts.depth || 0,
    long: opts.long,
    only,
    parseable: opts.parseable,
  }
  const output = args.length
    ? await listForPackages(args, prefix, listOpts)
    : await list(prefix, listOpts)

  if (output) console.log(output)
}
