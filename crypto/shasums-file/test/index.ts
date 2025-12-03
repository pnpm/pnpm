import { pickFileChecksumFromShasumsFile } from '@pnpm/crypto.shasums-file'

describe('pickFileChecksumFromShasumsFile', () => {
  it('picks the right checksum for a file', () => {
    expect(pickFileChecksumFromShasumsFile(`ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi`, 'foo.tar.gz')).toBe('sha256-7VIjkpStUX++kaJoFG1dKqihfS1i1khz5DIZB4unHE4=')
  })
  it('throws an error if no integrity found', () => {
    expect(() => pickFileChecksumFromShasumsFile(`ed52239294ad517fbe91a268146d5d2aa8a17d2d62d64873e43219078ba71c4e  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi`, 'bar.zip')).toThrow(/SHA-256 hash not found in SHASUMS256.txt for: bar.zip/)
  })
  it('throws an error if a malformed integrity is found', () => {
    expect(() => pickFileChecksumFromShasumsFile(`ed52239294ad517fbe91  foo.tar.gz
be127be1d98cad94c56f46245d0f2de89934d300028694456861a6d5ac558bf3  foo.msi`, 'foo.tar.gz')).toThrow(/Malformed SHA-256 for foo.tar.gz: ed52239294ad517fbe91/)
  })
})
