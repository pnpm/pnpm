import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import {
  extractMainDocument,
  streamReadFirstYamlDocument,
} from '../lib/yamlDocuments.js'

describe('streamReadFirstYamlDocument', () => {
  test('returns the first document content from a two-document file', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    fs.writeFileSync(filePath, '---\nfoo: bar\n---\nlockfileVersion: 9.0\n')
    const result = await streamReadFirstYamlDocument(filePath)
    expect(result).toBe('foo: bar')
  })

  test('returns null for a file that does not start with ---', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    fs.writeFileSync(filePath, 'lockfileVersion: 9.0\n')
    const result = await streamReadFirstYamlDocument(filePath)
    expect(result).toBeNull()
  })

  test('closes a non-env lockfile before returning null', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    const tempFilePath = `${filePath}.tmp`
    fs.writeFileSync(filePath, 'lockfileVersion: 9.0\n')
    fs.writeFileSync(tempFilePath, 'lockfileVersion: 9.0\nimporters: {}\n')

    const result = await streamReadFirstYamlDocument(filePath)

    expect(result).toBeNull()
    fs.renameSync(tempFilePath, filePath)
  })

  test('returns null for a non-existent file', async () => {
    const dir = temporaryDirectory()
    const result = await streamReadFirstYamlDocument(path.join(dir, 'nonexistent.yaml'))
    expect(result).toBeNull()
  })

  test('returns null when file starts with --- but has no second separator', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    fs.writeFileSync(filePath, '---\nfoo: bar\n')
    const result = await streamReadFirstYamlDocument(filePath)
    expect(result).toBeNull()
  })

  test('handles file with BOM prefix', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    fs.writeFileSync(filePath, '\uFEFF---\nfoo: bar\n---\nlockfileVersion: 9.0\n')
    const result = await streamReadFirstYamlDocument(filePath)
    expect(result).toBe('foo: bar')
  })

  test('handles BOM split across reads', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    fs.writeFileSync(filePath, '\uFEFF---\nfoo: bar\n---\nlockfileVersion: 9.0\n')
    const result = await streamReadFirstYamlDocument(filePath, 2)
    expect(result).toBe('foo: bar')
  })

  test.each([0, -1, Number.NaN])('falls back to default read buffer size for %p', async (readBufferSize) => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    fs.writeFileSync(filePath, '---\nfoo: bar\n---\nlockfileVersion: 9.0\n')
    const result = await streamReadFirstYamlDocument(filePath, readBufferSize)
    expect(result).toBe('foo: bar')
  })

  test('returns null for empty file', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    fs.writeFileSync(filePath, '')
    const result = await streamReadFirstYamlDocument(filePath)
    expect(result).toBeNull()
  })

  test('returns multiline first document content', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    const envContent = 'lockfileVersion: env-1.0\nimporters:\n  .:\n    foo: bar'
    fs.writeFileSync(filePath, `---\n${envContent}\n---\nlockfileVersion: 9.0\n`)
    const result = await streamReadFirstYamlDocument(filePath)
    expect(result).toBe(envContent)
  })

  test('handles CRLF line endings (Windows)', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    const envContent = 'lockfileVersion: env-1.0\nimporters:\n  .:\n    foo: bar'
    const content = `---\n${envContent}\n---\nlockfileVersion: 9.0\n`.replace(/\n/g, '\r\n')
    fs.writeFileSync(filePath, content)
    const result = await streamReadFirstYamlDocument(filePath)
    expect(result).toBe(envContent)
  })

  test('handles CRLF document start split across reads', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    fs.writeFileSync(filePath, '---\r\nfoo: bar\r\n---\r\nlockfileVersion: 9.0\r\n')
    const result = await streamReadFirstYamlDocument(filePath, 4)
    expect(result).toBe('foo: bar')
  })

  test('handles BOM with CRLF line endings', async () => {
    const dir = temporaryDirectory()
    const filePath = path.join(dir, 'test.yaml')
    const content = '﻿---\r\nfoo: bar\r\n---\r\nlockfileVersion: 9.0\r\n'
    fs.writeFileSync(filePath, content)
    const result = await streamReadFirstYamlDocument(filePath)
    expect(result).toBe('foo: bar')
  })
})

describe('extractMainDocument', () => {
  test('returns entire content when it does not start with ---', () => {
    const content = 'lockfileVersion: 9.0\npackages: {}\n'
    expect(extractMainDocument(content)).toBe(content)
  })

  test('returns empty string when content starts with --- but has no separator', () => {
    const content = '---\nfoo: bar\n'
    expect(extractMainDocument(content)).toBe('')
  })

  test('returns the second document from a combined file', () => {
    const mainContent = 'lockfileVersion: 9.0\npackages: {}\n'
    const combined = `---\nfoo: bar\n---\n${mainContent}`
    expect(extractMainDocument(combined)).toBe(mainContent)
  })

  test('handles CRLF line endings in combined file', () => {
    const mainContent = 'lockfileVersion: 9.0\npackages: {}\n'
    const combined = `---\nfoo: bar\n---\n${mainContent}`.replace(/\n/g, '\r\n')
    expect(extractMainDocument(combined)).toBe(mainContent)
  })

  test('normalizes CRLF to LF for content without document separator', () => {
    const content = 'lockfileVersion: 9.0\r\npackages: {}\r\n'
    expect(extractMainDocument(content)).toBe('lockfileVersion: 9.0\npackages: {}\n')
  })
})
