import crypto from 'crypto'

export type Hash = (algorithm: string, data: crypto.BinaryLike, outputEncoding: crypto.BinaryToTextEncoding) => string

export const hash: Hash =
  // @ts-expect-error -- crypto.hash is supported in Node 21.7.0+, 20.12.0+
  crypto.hash ??
  ((algorithm, data, outputEncoding) => crypto.createHash(algorithm).update(data).digest(outputEncoding))
