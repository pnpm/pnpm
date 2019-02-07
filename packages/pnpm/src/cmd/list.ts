import list, { forPackages as listForPackages } from 'pnpm-list'

export default async function (
  args: string[],
  opts: {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    development: boolean,
    global?: boolean,
    long?: boolean,
    parseable?: boolean,
    prefix: string,
    production: boolean,
    shrinkwrapDirectory?: string,
  },
  command: string,
) {
  if (opts.global && !('depth' in opts)) {
    // Copy opts as they're effectively frozen
    opts = JSON.parse(JSON.stringify(opts))
    opts.depth = 0
  }

  const output = await render(args, opts, command)

  if (output) console.log(output)
}

export async function render (
  args: string[],
  opts: {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    development: boolean,
    long?: boolean,
    parseable?: boolean,
    prefix: string,
    production: boolean,
    shrinkwrapDirectory?: string,
  },
  command: string,
) {
  opts.long = opts.long || command === 'll' || command === 'la'
  const only = (opts.production && opts.development ? undefined : (opts.production ? 'prod' : 'dev')) as ('prod' | 'dev' | undefined) // tslint:disable-line:no-unnecessary-type-assertion
  const listOpts = {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: opts.depth || 0,
    long: opts.long,
    only,
    parseable: opts.parseable,
    shrinkwrapDirectory: opts.shrinkwrapDirectory,
  }
  return args.length
    ? listForPackages(args, opts.prefix, listOpts)
    : list(opts.prefix, listOpts)
}
