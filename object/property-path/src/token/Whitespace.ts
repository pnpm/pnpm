import { type TokenBase, type Tokenize } from './types.js'

export interface Whitespace extends TokenBase {
  type: 'whitespace'
}

const WHITESPACE: Whitespace = { type: 'whitespace' }

export const parseWhitespace: Tokenize<Whitespace> = source => {
  const remaining = source.trimStart()
  return remaining === source ? undefined : [WHITESPACE, remaining]
}
