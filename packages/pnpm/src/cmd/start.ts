import run from './run'

export default async function (
  args: string[],
  opts: {
    prefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
  command: string,
) {
  return run(['start', ...args], opts, 'start')
}
