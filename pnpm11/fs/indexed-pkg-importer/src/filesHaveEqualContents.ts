import fs from 'node:fs'

const FILE_COMPARE_BUFFER_SIZE = 64 * 1024

export function filesHaveEqualContents (left: string, right: string): boolean {
  const leftBuffer = Buffer.allocUnsafe(FILE_COMPARE_BUFFER_SIZE)
  const rightBuffer = Buffer.allocUnsafe(FILE_COMPARE_BUFFER_SIZE)
  const leftFd = fs.openSync(left, 'r')
  try {
    const rightFd = fs.openSync(right, 'r')
    try {
      return compareOpenFiles(leftFd, rightFd, leftBuffer, rightBuffer)
    } finally {
      fs.closeSync(rightFd)
    }
  } finally {
    fs.closeSync(leftFd)
  }
}

function compareOpenFiles (leftFd: number, rightFd: number, leftBuffer: Buffer, rightBuffer: Buffer): boolean {
  for (;;) {
    const leftBytes = fs.readSync(leftFd, leftBuffer, 0, leftBuffer.length, null)
    const rightBytes = fs.readSync(rightFd, rightBuffer, 0, rightBuffer.length, null)
    if (leftBytes !== rightBytes) return false
    if (leftBytes === 0) return true
    if (!leftBuffer.subarray(0, leftBytes).equals(rightBuffer.subarray(0, rightBytes))) return false
  }
}
