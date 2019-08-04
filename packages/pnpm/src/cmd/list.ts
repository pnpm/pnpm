import { PnpmConfigs } from '@pnpm/config'
import list, { forPackages as listForPackages } from '@pnpm/list'

export default async function (
  args: string[],
  opts: PnpmConfigs & {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDirectory?: string,
    long?: boolean,
    parseable?: boolean,
    prefix: string,
  },
  command: string,
) {
  const output = await render(args, opts, command)

  if (output) console.log(output)
}

export async function render (
  args: string[],
  opts: PnpmConfigs & {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDirectory?: string,
    long?: boolean,
    json?: boolean,
    parseable?: boolean,
    prefix: string,
  },
  command: string,
) {
  opts.long = opts.long || command === 'll' || command === 'la'
  const listOpts = {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: opts.depth || 0,
    include: opts.include,
    lockfileDirectory: opts.lockfileDirectory,
    long: opts.long,
    reportAs: (opts.parseable ? 'parseable' : (opts.json ? 'json' : 'tree')) as ('parseable' | 'json' | 'tree'),
  }
  return args.length
    ? listForPackages(args, opts.prefix, listOpts)
    : list(opts.prefix, listOpts)
}
