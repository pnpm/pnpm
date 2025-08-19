import { type TokenBase, type Tokenize } from './types.js'

export const combineParsers = <Token extends TokenBase> (parsers: Iterable<Tokenize<Token>>): Tokenize<Token> => source => {
  for (const parse of parsers) {
    const parseResult = parse(source)
    if (parseResult) return parseResult
  }
  return undefined
}
