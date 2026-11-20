export const SPACE: number = ' '.charCodeAt(0)

/**
 * Parses an octal number at the specified `offset`, up to `length` characters. If it ends early, it will be terminated by either
 * a NUL or a space.
 */
export function parseOctal (buffer: Buffer, offset: number, length: number): number {
  const val = buffer.subarray(offset, offset + length)
  offset = 0

  // Older versions of tar can prefix with spaces
  while (offset < val.length && val[offset] === SPACE) offset++
  const end = clamp(indexOf(val, SPACE, offset, val.length), val.length, val.length)
  while (offset < end && val[offset] === 0) offset++
  if (end === offset) return 0
  return parseInt(val.subarray(offset, end).toString(), 8)
}

function indexOf (block: Buffer, num: number, offset: number, end: number): number {
  for (; offset < end; offset++) {
    if (block[offset] === num) return offset
  }
  return end
}

function clamp (index: number, len: number, defaultValue: number): number {
  if (typeof index !== 'number') return defaultValue
  index = ~~index // Coerce to integer.
  if (index >= len) return len
  if (index >= 0) return index
  index += len
  if (index >= 0) return index
  return 0
}
