import { type TokenBase, type Tokenize } from './types.js'

export interface Identifier extends TokenBase {
  type: 'identifier'
  content: string
}

export const parseIdentifier: Tokenize<Identifier> = source => {
  if (source === '') return undefined

  const firstChar = source[0]
  if (!/[a-z_]/i.test(firstChar)) return undefined

  let content = firstChar
  source = source.slice(1)
  while (source !== '') {
    const char = source[0]
    if (!/\w/.test(char)) break
    source = source.slice(1)
    content += char
  }

  return [{ type: 'identifier', content }, source]
}
