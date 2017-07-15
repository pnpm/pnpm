import list, {forPackages as listForPackages} from 'pnpm-list'

export default async function (
  args: string[],
  opts: {
    prefix: string,
    depth?: number,
    only?: 'dev' | 'prod',
    long?: boolean,
    parseable?: boolean,
  },
  command: string
) {
  opts.long = opts.long || command === 'll' || command === 'la'
  const output = args.length
    ? await listForPackages(args, opts.prefix, opts)
    : await list(opts.prefix, opts)
  console.log(output)
}
