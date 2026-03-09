import { jest, describe, it, expect, beforeEach, afterEach } from '@jest/globals'
import fs from 'fs'
import path from 'path'
import os from 'os'

jest.unstable_mockModule('@pnpm/exec.pnpm-cli-runner', () => ({
  runPnpmCli: jest.fn(),
}))

jest.unstable_mockModule('symlink-dir', () => ({
  default: { sync: jest.fn() },
}))

const { installPnpmToTools } = await import('../src/installPnpmToTools.js')
const { runPnpmCli } = await import('@pnpm/exec.pnpm-cli-runner')

type InstallOptions = Parameters<typeof installPnpmToTools>[1]

describe('installPnpmToTools', () => {
  let tempPnpmHome: string

  beforeEach(() => {
    jest.clearAllMocks()
    tempPnpmHome = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-test-home-'))
  })

  afterEach(() => {
    if (tempPnpmHome) {
      fs.rmSync(tempPnpmHome, { recursive: true, force: true })
    }
  })

  it('should include --config.minimum-release-age=0 to bypass age constraints for standalone binaries', async () => {
    const targetVersion = '10.30.1'

    await installPnpmToTools(targetVersion, {
      pnpmHomeDir: tempPnpmHome,
    } as InstallOptions)

    expect(runPnpmCli).toHaveBeenCalledTimes(1)

    const calledArgs = (runPnpmCli as jest.Mock).mock.calls[0][0]

    expect(calledArgs).toContain('--config.minimum-release-age=0')
    expect(calledArgs).toContain('--allow-build=@pnpm/exe')
    expect(calledArgs).toContain('--no-dangerously-allow-all-builds')
    expect(calledArgs).toContain('--config.node-linker=hoisted')
  })
})
