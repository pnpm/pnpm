import { removePnpmSpecificOptions } from '../lib/publish.js'

describe('removePnpmSpecificOptions', () => {
  it('should remove --no-git-checks', () => {
    const result = removePnpmSpecificOptions(['--no-git-checks', '--tag', 'latest'])
    expect(result).toEqual(['--tag', 'latest'])
  })

  it('should remove --publish-branch with its value', () => {
    const result = removePnpmSpecificOptions(['--publish-branch', 'main', '--tag', 'latest'])
    expect(result).toEqual(['--tag', 'latest'])
  })

  it('should remove --publish-branch without value (next is another option)', () => {
    const result = removePnpmSpecificOptions(['--publish-branch', '--tag', 'latest'])
    expect(result).toEqual(['--tag', 'latest'])
  })

  it('should remove --npm-path with its value', () => {
    const result = removePnpmSpecificOptions(['--npm-path', '/usr/bin/npm', '--tag', 'latest'])
    expect(result).toEqual(['--tag', 'latest'])
  })

  it('should preserve npm options', () => {
    const result = removePnpmSpecificOptions(['--tag', 'latest', '--access', 'public', '--dry-run'])
    expect(result).toEqual(['--tag', 'latest', '--access', 'public', '--dry-run'])
  })
})
