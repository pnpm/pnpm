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
    alwaysPrintRootPackage?: boolean,
  },
  command: string,
) {
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
    ? await listForPackages(args, opts.prefix, listOpts)
    : await list(opts.prefix, listOpts)

  if (output) console.log(output)
}
