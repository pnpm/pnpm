// The patterns below follow js-yaml 4, so that the lockfile quotes the scalars it always has.
// They also cover what js-yaml 5 reads as a number: a signed leading-dot float (`+.5`), and a
// number too large to be finite (`1e999`), which js-yaml 4 kept as a string.
const INTEGER_PATTERN = /^[-+]?(?:0b[01]+|0o[0-7]+|0x[\dA-Fa-f]+|\d+)$/
const FLOAT_PATTERN = /^(?:[-+]?\d+(?:\.\d*)?(?:[eE][-+]?\d+)?|[-+]?\.\d+(?:[eE][-+]?\d+)?|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$/
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/
const TIMESTAMP_PATTERN = /^\d{4}-\d{1,2}-\d{1,2}(?:[Tt]|[ \t]+)\d{1,2}:\d{2}:\d{2}(?:\.\d*)?(?:[ \t]*(?:Z|[-+]\d{1,2}(?::\d{2})?))?$/

const NULL_VALUES = new Set(['~', 'null', 'Null', 'NULL'])
const BOOLEAN_VALUES = new Set(['true', 'True', 'TRUE', 'false', 'False', 'FALSE'])

/**
 * Whether a plain scalar would be read back as a non-string: null, boolean,
 * integer, float, timestamp, or merge key.
 */
export function resolvesImplicitly (value: string): boolean {
  return NULL_VALUES.has(value) ||
    BOOLEAN_VALUES.has(value) ||
    INTEGER_PATTERN.test(value) ||
    FLOAT_PATTERN.test(value) ||
    DATE_PATTERN.test(value) ||
    TIMESTAMP_PATTERN.test(value) ||
    value === '<<'
}
