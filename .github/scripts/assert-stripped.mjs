// Release gate for the ELF and Mach-O binaries: a shipped binary carries no
// symbol table beyond the symbols the dynamic loader needs.
import { readFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

const ELF_MAGIC = Buffer.from([0x7f, 0x45, 0x4c, 0x46])
const ELF_CLASS_64 = 2
const ELF_DATA_LITTLE_ENDIAN = 1
const ELF_DATA_BIG_ENDIAN = 2
const SHT_SYMTAB = 2
const MACH_O_MAGIC_64 = 0xfeedfacf
const LC_DYSYMTAB = 0xb

/**
 * Describes the symbol table a strip would have removed, or returns
 * `undefined` when the binary has none. Throws on a file that is not a 64-bit
 * ELF or a thin 64-bit little-endian Mach-O.
 */
export function describeSymbolTable (bytes) {
  const view = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes)
  if (view.length >= 4 && view.subarray(0, 4).equals(ELF_MAGIC)) return describeElfSymbolTable(view)
  if (view.length >= 4 && view.readUInt32LE(0) === MACH_O_MAGIC_64) return describeMachOSymbolTable(view)
  throw new Error('not a 64-bit ELF or Mach-O file')
}

function describeElfSymbolTable (view) {
  if (view.length < 0x40) throw new Error('truncated ELF header')
  if (view[4] !== ELF_CLASS_64) throw new Error(`unsupported ELF class ${view[4]}`)
  const read = elfReader(view)
  const tableOffset = Number(read.uint64(0x28))
  const entrySize = read.uint16(0x3a)
  const entryCount = read.uint16(0x3c)
  if (tableOffset === 0) return undefined
  // A table of 0xff00 sections or more keeps its count in the first entry.
  if (entryCount === 0) throw new Error('unsupported ELF section count')
  if (entrySize < 0x40 || tableOffset + entrySize * entryCount > view.length) {
    throw new Error('truncated ELF section header table')
  }
  for (let index = 0; index < entryCount; index++) {
    if (read.uint32(tableOffset + index * entrySize + 4) === SHT_SYMTAB) return 'a SHT_SYMTAB section'
  }
  return undefined
}

function elfReader (view) {
  switch (view[5]) {
    case ELF_DATA_LITTLE_ENDIAN:
      return {
        uint16: offset => view.readUInt16LE(offset),
        uint32: offset => view.readUInt32LE(offset),
        uint64: offset => view.readBigUInt64LE(offset),
      }
    case ELF_DATA_BIG_ENDIAN:
      return {
        uint16: offset => view.readUInt16BE(offset),
        uint32: offset => view.readUInt32BE(offset),
        uint64: offset => view.readBigUInt64BE(offset),
      }
    default:
      throw new Error(`unsupported ELF data encoding ${view[5]}`)
  }
}

// A stripped Mach-O keeps LC_SYMTAB for its undefined and exported symbols, so
// the local symbol count in LC_DYSYMTAB is what tells the two apart.
function describeMachOSymbolTable (view) {
  if (view.length < 32) throw new Error('truncated Mach-O header')
  const commandCount = view.readUInt32LE(16)
  let command = 32
  for (let index = 0; index < commandCount; index++) {
    if (command + 8 > view.length) throw new Error('truncated Mach-O load command')
    const commandSize = view.readUInt32LE(command + 4)
    if (commandSize < 8 || command + commandSize > view.length) throw new Error('truncated Mach-O load command')
    if (view.readUInt32LE(command) === LC_DYSYMTAB) {
      if (commandSize < 16) throw new Error('truncated LC_DYSYMTAB')
      const localSymbolCount = view.readUInt32LE(command + 12)
      return localSymbolCount === 0 ? undefined : `${localSymbolCount} local symbols`
    }
    command += commandSize
  }
  throw new Error('Mach-O file has no LC_DYSYMTAB')
}

function main () {
  const files = process.argv.slice(2)
  if (files.length === 0) {
    console.error('usage: node assert-stripped.mjs <elf-or-mach-o-file>...')
    process.exit(2)
  }
  let failed = false
  for (const file of files) {
    let symbolTable
    try {
      symbolTable = describeSymbolTable(readFileSync(file))
    } catch (error) {
      console.error(`${file}: ${error.message}`)
      failed = true
      continue
    }
    if (symbolTable != null) {
      console.error(`${file} is not stripped: it has ${symbolTable}`)
      failed = true
    }
  }
  if (failed) process.exit(1)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
