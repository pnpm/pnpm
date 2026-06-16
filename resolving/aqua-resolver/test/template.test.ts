import { describe, expect, it } from '@jest/globals'

import { expandAssets, expandChecksumAssetName } from '../src/template.js'

describe('expandAssets', () => {
  it('expands ripgrep-style asset templates', () => {
    const assets = expandAssets('BurntSushi', 'ripgrep', '14.1.1', {
      version_constraint: 'true',
      asset: 'ripgrep-{{.Version}}-{{.Arch}}-{{.OS}}.{{.Format}}',
      format: 'tar.gz',
      files: [{ name: 'rg', src: 'ripgrep-{{.Version}}-{{.Arch}}-{{.OS}}/rg' }],
      replacements: {
        amd64: 'x86_64',
        arm64: 'aarch64',
        darwin: 'apple-darwin',
        windows: 'pc-windows-msvc',
      },
      overrides: [
        {
          goos: 'linux',
          goarch: 'amd64',
          replacements: { linux: 'unknown-linux-musl' },
        },
        {
          goos: 'linux',
          goarch: 'arm64',
          replacements: { linux: 'unknown-linux-gnu' },
        },
        {
          goos: 'windows',
          format: 'zip',
        },
      ],
    })

    const darwinArm = assets.find((a) => a.target.os === 'darwin' && a.target.cpu === 'arm64')
    expect(darwinArm).toBeDefined()
    expect(darwinArm!.url).toBe(
      'https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-aarch64-apple-darwin.tar.gz'
    )
    expect(darwinArm!.format).toBe('tar.gz')

    const linuxAmd = assets.find((a) => a.target.os === 'linux' && a.target.cpu === 'x64')
    expect(linuxAmd).toBeDefined()
    expect(linuxAmd!.url).toBe(
      'https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz'
    )

    const winAmd = assets.find((a) => a.target.os === 'win32' && a.target.cpu === 'x64')
    expect(winAmd).toBeDefined()
    expect(winAmd!.url).toBe(
      'https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-x86_64-pc-windows-msvc.zip'
    )
    expect(winAmd!.format).toBe('zip')
  })

  it('expands fzf-style asset templates with trimV', () => {
    const assets = expandAssets('junegunn', 'fzf', 'v0.57.0', {
      version_constraint: 'true',
      asset: 'fzf-{{trimV .Version}}-{{.OS}}_{{.Arch}}.{{.Format}}',
      format: 'tar.gz',
      overrides: [
        { goos: 'windows', format: 'zip' },
      ],
    })

    const darwinArm = assets.find((a) => a.target.os === 'darwin' && a.target.cpu === 'arm64')
    expect(darwinArm).toBeDefined()
    expect(darwinArm!.url).toBe(
      'https://github.com/junegunn/fzf/releases/download/v0.57.0/fzf-0.57.0-darwin_arm64.tar.gz'
    )

    const winAmd = assets.find((a) => a.target.os === 'win32' && a.target.cpu === 'x64')
    expect(winAmd).toBeDefined()
    expect(winAmd!.url).toBe(
      'https://github.com/junegunn/fzf/releases/download/v0.57.0/fzf-0.57.0-windows_amd64.zip'
    )
  })

  it('filters platforms based on supported_envs', () => {
    const assets = expandAssets('test', 'tool', 'v1.0.0', {
      version_constraint: 'true',
      asset: 'tool-{{.OS}}-{{.Arch}}.tar.gz',
      format: 'tar.gz',
      supported_envs: ['linux/amd64', 'darwin'],
    })

    const oses = assets.map((a) => `${a.target.os}/${a.target.cpu}`)
    expect(oses).toContain('linux/x64')
    expect(oses).toContain('darwin/arm64')
    expect(oses).toContain('darwin/x64')
    expect(oses).not.toContain('win32/x64')
    expect(oses).not.toContain('linux/arm64')
  })
})

describe('expandChecksumAssetName', () => {
  it('expands checksum asset template with .Asset', () => {
    const result = expandChecksumAssetName(
      '{{.Asset}}.sha256',
      'ripgrep-14.1.1-x86_64-apple-darwin.tar.gz',
      '14.1.1'
    )
    expect(result).toBe('ripgrep-14.1.1-x86_64-apple-darwin.tar.gz.sha256')
  })

  it('expands checksum asset template with trimV', () => {
    const result = expandChecksumAssetName(
      'fzf_{{trimV .Version}}_checksums.txt',
      'fzf-0.57.0-darwin_arm64.tar.gz',
      'v0.57.0'
    )
    expect(result).toBe('fzf_0.57.0_checksums.txt')
  })
})
