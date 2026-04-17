#!/usr/bin/env node
'use strict'

// Microbenchmark comparing the native Rust writer against the JS parse+write
// path used by worker/src/start.ts fetchAndWriteCafs (minus the HTTP/gunzip
// framing, so this measures parse + write only).
//
// Run: node bench/compare.js [FILE_COUNT] [FILE_SIZE_BYTES] [ITERATIONS]

const crypto = require('node:crypto')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')

const { writeFiles: nativeWriteFiles } = require('..')

const FILE_COUNT = parseInt(process.argv[2] ?? '5000', 10)
const FILE_SIZE = parseInt(process.argv[3] ?? '4096', 10)
const ITERATIONS = parseInt(process.argv[4] ?? '3', 10)

function buildPayload (fileCount, fileSize) {
  const parts = []
  const lenBuf = Buffer.alloc(4)
  lenBuf.writeUInt32BE(2, 0)
  parts.push(lenBuf, Buffer.from('{}'))

  for (let i = 0; i < fileCount; i++) {
    // Unique content per file so digests are unique
    const content = crypto.randomBytes(fileSize)
    const digest = crypto.hash('sha512', content, 'hex')
    parts.push(Buffer.from(digest, 'hex'))
    const sizeBuf = Buffer.alloc(4)
    sizeBuf.writeUInt32BE(fileSize, 0)
    parts.push(sizeBuf, Buffer.from([i & 1 ? 0x01 : 0x00]), content)
  }
  parts.push(Buffer.alloc(64, 0))
  return Buffer.concat(parts)
}

function jsWriteFiles (storeDir, payload) {
  let pos = 0
  const jsonLen = payload.readUInt32BE(pos); pos += 4
  pos += jsonLen

  const END_MARKER = Buffer.alloc(64, 0)
  const createdDirs = new Set()
  let written = 0

  while (pos < payload.length) {
    const digestBuf = payload.subarray(pos, pos + 64)
    if (digestBuf.equals(END_MARKER)) break
    pos += 64
    const size = payload.readUInt32BE(pos); pos += 4
    const executable = (payload[pos] & 0x01) !== 0; pos += 1
    const content = payload.subarray(pos, pos + size); pos += size

    const digest = digestBuf.toString('hex')
    const dir = path.join(storeDir, 'files', digest.slice(0, 2))
    const file = path.join(dir, executable ? `${digest.slice(2)}-exec` : digest.slice(2))
    if (!createdDirs.has(dir)) {
      fs.mkdirSync(dir, { recursive: true })
      createdDirs.add(dir)
    }
    try {
      fs.writeFileSync(file, content, { flag: 'wx', mode: executable ? 0o755 : 0o644 })
      written++
    } catch (err) {
      if (err.code !== 'EEXIST') throw err
    }
  }
  return written
}

function bench (label, fn) {
  const tmpBase = fs.mkdtempSync(path.join(os.tmpdir(), 'cafs-bench-'))
  try {
    const start = process.hrtime.bigint()
    const written = fn(tmpBase)
    const ms = Number(process.hrtime.bigint() - start) / 1_000_000
    console.log(`  ${label.padEnd(10)} ${ms.toFixed(1)}ms  (${written} files written)`)
    return ms
  } finally {
    fs.rmSync(tmpBase, { recursive: true, force: true })
  }
}

console.log(`Files: ${FILE_COUNT}  size: ${FILE_SIZE} bytes  iterations: ${ITERATIONS}`)
const payload = buildPayload(FILE_COUNT, FILE_SIZE)
console.log(`Payload size: ${(payload.length / 1024 / 1024).toFixed(1)} MB\n`)

for (let i = 0; i < ITERATIONS; i++) {
  console.log(`Iteration ${i + 1}:`)
  bench('native', (dir) => nativeWriteFiles(dir, payload))
  bench('js', (dir) => jsWriteFiles(dir, payload))
  console.log()
}
