import { execSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { removeQuarantine, hasQuarantine } from '../src/removeQuarantine.js'

// Only run these tests on macOS where quarantine xattrs exist
const describeOnMacOS = process.platform === 'darwin' ? describe : describe.skip

describeOnMacOS('removeQuarantine', () => {
  const testDir = path.join(__dirname, '__tmp__', 'quarantine-test')
  const testFile = path.join(testDir, 'test-file.txt')

  beforeEach(() => {
    fs.mkdirSync(testDir, { recursive: true })
    fs.writeFileSync(testFile, 'test content')
  })

  afterEach(() => {
    fs.rmSync(testDir, { recursive: true, force: true })
  })

  it('should remove quarantine xattr from a file', () => {
    // Add quarantine xattr
    execSync(`/usr/bin/xattr -w com.apple.quarantine "0083;$(printf '%x' $(date +%s));TestApp;" "${testFile}"`)
    
    // Verify it exists
    expect(hasQuarantine(testFile)).toBe(true)
    
    // Remove it
    const result = removeQuarantine(testFile)
    expect(result).toBe(true)
    
    // Verify it's gone
    expect(hasQuarantine(testFile)).toBe(false)
  })

  it('should succeed when quarantine xattr does not exist', () => {
    // Verify no quarantine initially
    expect(hasQuarantine(testFile)).toBe(false)
    
    // Try to remove (should succeed even though nothing to remove)
    const result = removeQuarantine(testFile)
    expect(result).toBe(true)
  })

  it('should handle files with multiple xattrs', () => {
    // Add multiple xattrs including quarantine
    execSync(`/usr/bin/xattr -w com.apple.quarantine "0083;$(printf '%x' $(date +%s));TestApp;" "${testFile}"`)
    execSync(`/usr/bin/xattr -w com.example.custom "test-value" "${testFile}"`)
    
    // Verify quarantine exists
    expect(hasQuarantine(testFile)).toBe(true)
    
    // Remove quarantine
    const result = removeQuarantine(testFile)
    expect(result).toBe(true)
    
    // Verify quarantine is gone
    expect(hasQuarantine(testFile)).toBe(false)
    
    // Verify other xattr remains
    const output = execSync(`/usr/bin/xattr -l "${testFile}"`, { encoding: 'utf8' })
    expect(output).toContain('com.example.custom')
    expect(output).not.toContain('com.apple.quarantine')
  })
})
