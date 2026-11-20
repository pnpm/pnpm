// The patterns below are copied from js-yaml 4, so that the lockfile quotes exactly the scalars it always has.
const FLOAT_PATTERN = /^(?:[-+]?\d+(?:\.\d*)?(?:[eE][-+]?\d+)?|\.\d+(?:[eE][-+]?\d+)?|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$/
const FLOAT_SPECIAL_PATTERN = /^(?:[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$/
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/
const TIMESTAMP_PATTERN = /^\d{4}-\d{1,2}-\d{1,2}(?:[Tt]|[ \t]+)\d{1,2}:\d{2}:\d{2}(?:\.\d*)?(?:[ \t]*(?:Z|[-+]\d{1,2}(?::\d{2})?))?$/

const NULL_VALUES = new Set(['~', 'null', 'Null', 'NULL'])
const BOOLEAN_VALUES = new Set(['true', 'True', 'TRUE', 'false', 'False', 'FALSE'])

const RADIX_BY_PREFIX: Record<string, number> = { b: 2, o: 8, x: 16 }

/**
 * Whether a plain scalar would be read back as a non-string by js-yaml 4's
 * default schema: null, boolean, integer, float, timestamp, or merge key.
 */
export function resolvesImplicitly (value: string): boolean {
  return NULL_VALUES.has(value) ||
    BOOLEAN_VALUES.has(value) ||
    resolvesInteger(value) ||
    resolvesFloat(value) ||
    DATE_PATTERN.test(value) ||
    TIMESTAMP_PATTERN.test(value) ||
    value === '<<'
}

function resolvesInteger (value: string): boolean {
  const unsigned = value.startsWith('-') || value.startsWith('+') ? value.slice(1) : value
  if (unsigned === '0') return true
  const radix = unsigned.startsWith('0') ? RADIX_BY_PREFIX[unsigned[1]] : undefined
  const digits = radix == null ? unsigned : unsigned.slice(2)
  if (digits === '' || ![...digits].every((digit) => isDigitOfRadix(digit, radix ?? 10))) return false
  return Number.isFinite(Number.parseInt(digits, radix ?? 10))
}

function isDigitOfRadix (digit: string, radix: number): boolean {
  return !Number.isNaN(Number.parseInt(digit, radix))
}

function resolvesFloat (value: string): boolean {
  if (!FLOAT_PATTERN.test(value)) return false
  return Number.isFinite(Number.parseFloat(value)) || FLOAT_SPECIAL_PATTERN.test(value)
}
