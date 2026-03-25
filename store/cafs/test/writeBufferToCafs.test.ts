import { execSync } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

import { temporaryDirectory } from 'tempy'

import { writeBufferToCafs } from '../src/writeBufferToCafs.js'

const testDir = path.dirname(fileURLToPath(import.meta.url))

describe('writeBufferToCafs', () => {
  it('should write directly to the final CAS path', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', buffer, 'hex')
    writeBufferToCafs(new Map(), storeDir, buffer, fileDest, 420, { digest, algorithm: 'sha512' })
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe('abc')
  })

  it('should handle EEXIST when the existing file has correct integrity', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', buffer, 'hex')
    const integrity = { digest, algorithm: 'sha512' }
    const locker = new Map<string, number>()

    // Simulate another process creating the file before our exclusive write
    fs.mkdirSync(path.dirname(fullFileDest), { recursive: true })
    fs.writeFileSync(fullFileDest, buffer)

    // Our call should find the file via stat, verify integrity, and cache it
    const result = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    expect(result.filePath).toBe(fullFileDest)
    expect(locker.has(fullFileDest)).toBe(true)
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe('abc')
  })

  it('should overwrite an existing file with wrong integrity without deleting it first', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', buffer, 'hex')
    const integrity = { digest, algorithm: 'sha512' }
    const locker = new Map<string, number>()

    // Create a file with wrong content (simulating corruption or partial write)
    fs.mkdirSync(path.dirname(fullFileDest), { recursive: true })
    fs.writeFileSync(fullFileDest, 'wrong content')

    // Should detect mismatch and overwrite (not delete + recreate)
    const result = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    expect(result.filePath).toBe(fullFileDest)
    expect(locker.has(fullFileDest)).toBe(true)
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe('abc')
  })

  it('should handle concurrent writes from multiple processes without corruption', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    // Use a 256 KB buffer so writeFileSync issues multiple write() syscalls,
    // increasing the window for concurrent-write races.
    const content = crypto.randomBytes(256 * 1024)
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', content, 'hex')

    const libUrl = pathToFileURL(path.resolve(testDir, '../lib/writeBufferToCafs.js')).href
    const resultsDir = path.join(storeDir, '_results')
    fs.mkdirSync(resultsDir)

    // Each worker writes the same CAS content and saves its return value to a
    // per-process JSON file so we can verify every worker got a valid result.
    const workerScript = path.join(storeDir, '_worker.mjs')
    fs.writeFileSync(workerScript, `
      import fs from 'node:fs';
      import path from 'node:path';
      import { writeBufferToCafs } from ${JSON.stringify(libUrl)};
      const content = Buffer.from(${JSON.stringify(content.toString('base64'))}, 'base64');
      const locker = new Map();
      const result = writeBufferToCafs(locker, ${JSON.stringify(storeDir)}, content, ${JSON.stringify(fileDest)}, 420, { digest: ${JSON.stringify(digest)}, algorithm: 'sha512' });
      fs.writeFileSync(path.join(${JSON.stringify(resultsDir)}, process.pid + '.json'), JSON.stringify(result));
    `)

    const spawnerScript = path.join(storeDir, '_spawner.mjs')
    fs.writeFileSync(spawnerScript, `
      import { fork } from 'node:child_process';
      const N = 8;
      const workerScript = process.argv[2];
      const children = [];
      for (let i = 0; i < N; i++) {
        children.push(new Promise((resolve, reject) => {
          const p = fork(workerScript, [], { stdio: 'pipe' });
          let stderr = '';
          p.stderr.on('data', d => { stderr += d; });
          p.on('exit', code => code === 0 ? resolve() : reject(new Error('Process ' + i + ' exited ' + code + ': ' + stderr)));
        }));
      }
      const results = await Promise.allSettled(children);
      const failures = results.filter(r => r.status === 'rejected');
      if (failures.length > 0) {
        for (const f of failures) console.error(f.reason.message);
        process.exit(1);
      }
    `)

    execSync(`node ${JSON.stringify(spawnerScript)} ${JSON.stringify(workerScript)}`, {
      timeout: 30000,
      stdio: 'pipe',
    })

    // Every worker must have completed without throwing
    const resultFiles = fs.readdirSync(resultsDir)
    expect(resultFiles).toHaveLength(8)

    // Every worker must have returned a valid result pointing to the correct path
    for (const file of resultFiles) {
      const result = JSON.parse(fs.readFileSync(path.join(resultsDir, file), 'utf8'))
      expect(result.filePath).toBe(fullFileDest)
      expect(typeof result.checkedAt).toBe('number')
    }

    // The final file on disk must have correct content and integrity
    const finalContent = fs.readFileSync(fullFileDest)
    expect(finalContent).toHaveLength(content.length)
    expect(crypto.hash('sha512', finalContent, 'hex')).toBe(digest)
  })

  it('should populate the locker cache when a file already exists with correct integrity', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const digest = crypto.hash('sha512', buffer, 'hex')
    const integrity = { digest, algorithm: 'sha512' }
    const locker = new Map<string, number>()

    // First write creates the file
    writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    // Clear the locker to simulate a fresh lookup
    locker.clear()

    // Second call should find the file on disk and cache it
    const result = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    const fullFileDest = path.join(storeDir, fileDest)
    expect(locker.get(fullFileDest)).toBe(result.checkedAt)

    // Third call should return from locker cache without hitting disk
    const cached = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    expect(cached.checkedAt).toBe(result.checkedAt)
  })
})
