export interface TokenBase {
  type: string
}

/**
* Extract a token from a source.
* @param source The source string.
* @returns The token and the remaining unparsed string.
*/
export type Tokenize<Token extends TokenBase> = (source: string) => [Token, string] | undefined
