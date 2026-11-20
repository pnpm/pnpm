import fs from 'node:fs'

import Bunzip from 'seek-bzip'

import type { TarballParser } from './parseTarball.js'

const CHUNK_SIZE = 128 * 1024

export interface Bzip2Input {
  readByte: () => number
}

export function decodeBzip2 (input: Uint8Array | Bzip2Input, parser: TarballParser): void {
  let chunk = Buffer.allocUnsafe(CHUNK_SIZE)
  let written = 0
  Bunzip.decode(createInputStream(input), {
    writeByte: (byte) => {
      chunk[written++] = byte
      if (written === chunk.length) {
        parser.push(chunk)
        chunk = Buffer.allocUnsafe(CHUNK_SIZE)
        written = 0
      }
    },
  })
  if (written > 0) parser.push(chunk.subarray(0, written))
  parser.end()
}

export function openBzip2Input (filename: string): { input: Bzip2Input, close: () => void } {
  const descriptor = fs.openSync(filename, 'r')
  const chunk = Buffer.allocUnsafe(CHUNK_SIZE)
  let available = 0
  let offset = 0
  return {
    input: {
      readByte: () => {
        if (offset === available) {
          available = fs.readSync(descriptor, chunk)
          offset = 0
        }
        return offset < available ? chunk[offset++] : -1
      },
    },
    close: () => fs.closeSync(descriptor),
  }
}

function createInputStream (input: Uint8Array | Bzip2Input): { readByte: () => number, read: (buffer: Uint8Array, offset: number, length: number) => number } {
  const stream = 'readByte' in input ? input : createBufferInput(input)
  return {
    readByte: stream.readByte,
    read: (buffer, offset, length) => {
      let read = 0
      while (read < length) {
        const byte = stream.readByte()
        if (byte < 0) break
        buffer[offset + read++] = byte
      }
      return read === 0 ? -1 : read
    },
  }
}

function createBufferInput (input: Uint8Array): Bzip2Input {
  let offset = 0
  return { readByte: () => offset < input.length ? input[offset++] : -1 }
}
