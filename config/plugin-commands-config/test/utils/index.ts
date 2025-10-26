import { type config } from '../../src/index.js'

export function getOutputString (result: config.ConfigHandlerResult): string {
  if (result == null) throw new Error('output is null or undefined')
  if (typeof result === 'string') return result
  if (typeof result === 'object') return result.output
  const _typeGuard: never = result // eslint-disable-line @typescript-eslint/no-unused-vars
  throw new Error('unreachable')
}
