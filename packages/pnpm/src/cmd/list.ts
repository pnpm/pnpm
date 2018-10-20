import list, { forPackages as listForPackages } from 'pnpm-list'

export default async function (
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
  const output = args.length
    ? await listForPackages(args, opts.prefix, listOpts)
    : await list(opts.prefix, listOpts)

  if (output) console.log(output)
}
