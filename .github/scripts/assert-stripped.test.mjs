import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { describeSymbolTable } from './assert-stripped.mjs'

const script = fileURLToPath(new URL('./assert-stripped.mjs', import.meta.url))

const SHT_PROGBITS = 1
const SHT_SYMTAB = 2
const SHT_STRTAB = 3
const SHT_DYNSYM = 11

test('an ELF with only a dynamic symbol table is stripped', () => {
  for (const littleEndian of [true, false]) {
    const bytes = elfWithSections([SHT_PROGBITS, SHT_DYNSYM, SHT_STRTAB], { littleEndian })
    assert.equal(describeSymbolTable(bytes), undefined)
    assert.equal(run(bytes), 0)
  }
})

test('an ELF with a SHT_SYMTAB section is rejected in either byte order', () => {
  for (const littleEndian of [true, false]) {
    const bytes = elfWithSections([SHT_PROGBITS, SHT_DYNSYM, SHT_SYMTAB, SHT_STRTAB], { littleEndian })
    assert.equal(describeSymbolTable(bytes), 'a SHT_SYMTAB section')
    assert.equal(run(bytes), 1)
  }
})

test('an ELF without section headers is stripped', () => {
  const bytes = elfWithSections([])
  bytes.writeBigUInt64LE(0n, 0x28)
  bytes.writeUInt16LE(0, 0x3c)
  assert.equal(describeSymbolTable(bytes), undefined)
})

test('an ELF that keeps its section count outside the file header fails the check', () => {
  const bytes = elfWithSections([SHT_PROGBITS, SHT_SYMTAB])
  bytes.writeUInt16LE(0, 0x3c)
  assert.throws(() => describeSymbolTable(bytes), /unsupported ELF section count/)
  assert.equal(run(bytes), 1)
})

test('an ELF whose section header table runs past the file fails the check', () => {
  const bytes = elfWithSections([SHT_PROGBITS, SHT_SYMTAB]).subarray(0, 0x40 + 0x40)
  assert.throws(() => describeSymbolTable(bytes), /truncated ELF section header table/)
  assert.equal(run(bytes), 1)
})

test('SHT_SYMTAB detection matches readelf', () => {
  for (const sectionTypes of [[SHT_PROGBITS, SHT_DYNSYM], [SHT_PROGBITS, SHT_SYMTAB]]) {
    const bytes = elfWithSections(sectionTypes)
    const file = writeSample(bytes)
    const dump = execFileSync('readelf', ['--section-headers', '--wide', file], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
    assert.equal(describeSymbolTable(bytes) != null, /\bSYMTAB\b/.test(dump))
  }
})

test('a Mach-O without local symbols is stripped', () => {
  const bytes = machOWithLocalSymbols(0)
  assert.equal(describeSymbolTable(bytes), undefined)
  assert.equal(run(bytes), 0)
})

test('a Mach-O with local symbols is rejected', () => {
  const bytes = machOWithLocalSymbols(52909)
  assert.equal(describeSymbolTable(bytes), '52909 local symbols')
  assert.equal(run(bytes), 1)
})

test('a Mach-O without LC_DYSYMTAB fails the check', () => {
  const bytes = machOWithLocalSymbols(0)
  bytes.writeUInt32LE(1, 16)
  assert.throws(() => describeSymbolTable(bytes), /no LC_DYSYMTAB/)
  assert.equal(run(bytes), 1)
})

test('a Mach-O load command that runs past the file fails the check', () => {
  const bytes = machOWithLocalSymbols(0).subarray(0, 32 + 24 + 40)
  assert.throws(() => describeSymbolTable(bytes), /truncated Mach-O load command/)
  assert.equal(run(bytes), 1)
})

test('a file that is neither ELF nor Mach-O fails the check', () => {
  assert.throws(() => describeSymbolTable(Buffer.from('MZ')), /not a 64-bit ELF or Mach-O file/)
  assert.equal(run(Buffer.from('#!/bin/sh\n')), 1)
})

function run (bytes) {
  try {
    execFileSync(process.execPath, [script, writeSample(bytes)], { stdio: 'pipe' })
    return 0
  } catch (error) {
    return error.status ?? 1
  }
}

function writeSample (bytes) {
  const dir = mkdtempSync(path.join(tmpdir(), 'pnpm-stripped-'))
  const file = path.join(dir, 'sample')
  writeFileSync(file, bytes)
  return file
}

// Minimal ELF64: a file header followed by the null section header and one
// empty section per requested type.
function elfWithSections (sectionTypes, { littleEndian = true } = {}) {
  const headerSize = 0x40
  const sectionHeaderSize = 0x40
  const types = [0, ...sectionTypes]
  const file = Buffer.alloc(headerSize + types.length * sectionHeaderSize)
  const writeUInt16 = (value, offset) => littleEndian ? file.writeUInt16LE(value, offset) : file.writeUInt16BE(value, offset)
  const writeUInt32 = (value, offset) => littleEndian ? file.writeUInt32LE(value, offset) : file.writeUInt32BE(value, offset)
  const writeUInt64 = (value, offset) => littleEndian ? file.writeBigUInt64LE(BigInt(value), offset) : file.writeBigUInt64BE(BigInt(value), offset)
  Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, littleEndian ? 1 : 2, 1]).copy(file)
  writeUInt16(2, 0x10)
  writeUInt16(littleEndian ? 0x3e : 0x16, 0x12)
  writeUInt32(1, 0x14)
  writeUInt64(headerSize, 0x28)
  writeUInt16(headerSize, 0x34)
  writeUInt16(sectionHeaderSize, 0x3a)
  writeUInt16(types.length, 0x3c)
  for (const [index, type] of types.entries()) {
    const sectionHeader = headerSize + index * sectionHeaderSize
    writeUInt32(type, sectionHeader + 4)
    writeUInt64(file.length, sectionHeader + 0x18)
  }
  return file
}

// Minimal thin 64-bit Mach-O with the two symbol-table load commands.
function machOWithLocalSymbols (localSymbolCount) {
  const symtabSize = 24
  const dysymtabSize = 80
  const file = Buffer.alloc(32 + symtabSize + dysymtabSize)
  file.writeUInt32LE(0xfeedfacf, 0)
  file.writeUInt32LE(0x0100000c, 4)
  file.writeUInt32LE(2, 12)
  file.writeUInt32LE(2, 16)
  file.writeUInt32LE(symtabSize + dysymtabSize, 20)
  file.writeUInt32LE(0x2, 32)
  file.writeUInt32LE(symtabSize, 36)
  const dysymtab = 32 + symtabSize
  file.writeUInt32LE(0xb, dysymtab)
  file.writeUInt32LE(dysymtabSize, dysymtab + 4)
  file.writeUInt32LE(localSymbolCount, dysymtab + 12)
  return file
}
