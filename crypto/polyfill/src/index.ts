import crypto from 'crypto'

// Overload signatures:
export function hash (
  algorithm: string,
  data: crypto.BinaryLike
): Buffer
export function hash (
  algorithm: string,
  data: crypto.BinaryLike,
  outputEncoding: crypto.BinaryToTextEncoding
): string

// Implementation signature:
export function hash (
  algorithm: string,
  data: crypto.BinaryLike,
  outputEncoding?: crypto.BinaryToTextEncoding
): string | Buffer {
  // @ts-expect-error -- crypto.hash is supported in Node 21.7.0+, 20.12.0+
  if (crypto.hash) {
    // @ts-expect-error -- crypto.hash is supported in Node 21.7.0+, 20.12.0+
    // https://nodejs.org/docs/latest/api/crypto.html#cryptohashalgorithm-data-outputencoding
    // crypto.hash outputEncoding is optional and defaults to 'hex', we should treat it as digest without receiving parameter
    return outputEncoding ? crypto.hash(algorithm, data, outputEncoding) : crypto.hash(algorithm, data, 'buffer')
  }
  // Fallback to createHash:
  const h = crypto.createHash(algorithm)
  h.update(data)
  return outputEncoding ? h.digest(outputEncoding) : h.digest()
}
