import list, {forPackages as listForPackages} from 'pnpm-list'

export default async function (
  args: string[],
  opts: {
    depth?: number,
    only?: 'dev' | 'prod',
    long?: boolean,
    parseable?: boolean,
  },
  command: string
) {
  opts.long = opts.long || command === 'll' || command === 'la'
  const cwd = process.cwd()
  const output = args.length
    ? await listForPackages(args, cwd, opts)
    : await list(cwd, opts)
  console.log(output)
}
