import crypto from 'crypto'

export const hash =
  // @ts-expect-error -- crypto.hash is supported in Node 21.7.0+, 20.12.0+
  crypto.hash ??
  ((
    algorithm: string,
    data: crypto.BinaryLike,
    outputEncoding: crypto.BinaryToTextEncoding
  ) => crypto.createHash(algorithm).update(data).digest(outputEncoding))
