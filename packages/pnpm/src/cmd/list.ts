import { Config } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import list, { forPackages as listForPackages } from '@pnpm/list'

export default async function (
  args: string[],
  opts: Config & {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDirectory?: string,
    long?: boolean,
    parseable?: boolean,
    prefix: string,
  },
  command: string,
) {
  const output = await render([opts.prefix], args, {
    ...opts,
    lockfileDirectory: opts.lockfileDirectory || opts.prefix,
  }, command)

  if (output) console.log(output)
}

export async function render (
  prefixes: string[],
  args: string[],
  opts: Config & {
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDirectory: string,
    long?: boolean,
    json?: boolean,
    parseable?: boolean,
  },
  command: string,
) {
  const isWhy = command === 'why'
  if (isWhy && !args.length) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm why` requires the package name')
  }
  opts.long = opts.long || command === 'll' || command === 'la'
  const listOpts = {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: isWhy ? Infinity : opts.depth || 0,
    include: opts.include,
    lockfileDirectory: opts.lockfileDirectory,
    long: opts.long,
    // tslint:disable-next-line: no-unnecessary-type-assertion
    reportAs: (opts.parseable ? 'parseable' : (opts.json ? 'json' : 'tree')) as ('parseable' | 'json' | 'tree'),
  }
  return isWhy || args.length
    ? listForPackages(args, prefixes, listOpts)
    : list(prefixes, listOpts)
}
