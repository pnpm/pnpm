import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, it, jest } from '@jest/globals'

jest.unstable_mockModule('@inquirer/prompts', () => {
  class Separator {
    separator: string
    readonly type = 'separator' as const
    constructor (separator: string) {
      this.separator = separator
    }
  }
  return {
    Separator,
    checkbox: jest.fn(),
    confirm: jest.fn(),
    input: jest.fn(),
    password: jest.fn(),
    select: jest.fn(),
  }
})
const { checkbox, input } = await import('@inquirer/prompts')
const { change } = await import('../../src/index.js')

const mockCheckbox = jest.mocked(checkbox)
const mockInput = jest.mocked(input)

let tempDir: string
let opts: object

beforeEach(() => {
  tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-change-cancel-test-'))
  fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  const rootDir = path.join(tempDir, 'packages', 'lib')
  fs.mkdirSync(rootDir, { recursive: true })
  const manifest = { name: 'lib', version: '1.0.0' }
  fs.writeFileSync(path.join(rootDir, 'package.json'), JSON.stringify(manifest, null, 2))
  opts = { dir: tempDir, workspaceDir: tempDir, allProjects: [{ rootDir, manifest }] }
})

afterEach(() => {
  fs.rmSync(tempDir, { recursive: true, force: true })
  mockCheckbox.mockReset()
  mockInput.mockReset()
})

function exitPromptError (): Error {
  const err = new Error('User force closed the prompt')
  err.name = 'ExitPromptError'
  return err
}

// `process.exit()` never returns in production, so the spy throws to stop the
// handler where the real call would.
async function expectExit (): Promise<void> {
  const exited = new Error('process.exit')
  const exitSpy = jest.spyOn(process, 'exit').mockImplementation(() => {
    throw exited
  })
  try {
    await expect(change.handler(opts as any, [])).rejects.toBe(exited) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(exitSpy).toHaveBeenCalledWith(0)
  } finally {
    exitSpy.mockRestore()
  }
}

it('change leaves without an error when the package prompt is canceled', async () => {
  mockCheckbox.mockRejectedValue(exitPromptError())
  await expectExit()
})

it('change leaves without an error when a bump prompt is canceled', async () => {
  mockCheckbox.mockResolvedValueOnce(['lib']).mockRejectedValue(exitPromptError())
  await expectExit()
})

it('change leaves without an error when the summary prompt is canceled', async () => {
  mockCheckbox.mockResolvedValueOnce(['lib']).mockResolvedValue([])
  mockInput.mockRejectedValue(exitPromptError())
  await expectExit()
})

it('change rethrows a prompt failure that is not a cancellation', async () => {
  const failed = new Error('the terminal is not interactive')
  mockCheckbox.mockRejectedValue(failed)
  await expect(change.handler(opts as any, [])).rejects.toBe(failed) // eslint-disable-line @typescript-eslint/no-explicit-any
})
