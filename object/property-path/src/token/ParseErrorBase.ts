import { PnpmError } from '@pnpm/error'

/**
 * Base class for all parser errors.
 * This allows consumer code to detect a parser error by simply checking `instanceof`.
 */
export abstract class ParseErrorBase extends PnpmError {}
