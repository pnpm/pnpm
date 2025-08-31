import { type TokenBase, type Tokenize } from './types.js'

export interface ExactToken<Content extends string> extends TokenBase {
  type: 'exact'
  content: Content
}

const createExactTokenParser =
  <Content extends string>(content: Content): Tokenize<ExactToken<Content>> =>
    source => source.startsWith(content) ? [{ type: 'exact', content }, source.slice(content.length)] : undefined

export const parseDotOperator = createExactTokenParser('.')
export const parseOpenBracket = createExactTokenParser('[')
export const parseCloseBracket = createExactTokenParser(']')
