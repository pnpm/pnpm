#!/usr/bin/env node
import PnpmError from '@pnpm/error'
import findWorkspaceDir from '@pnpm/find-workspace-dir'
import makeDedicatedLockfile from '.'

main() // eslint-disable-line

async function main () {
  const projectDir = process.cwd()
  const lockfileDir = await findWorkspaceDir(projectDir)
  if (!lockfileDir) {
    throw new PnpmError('WORKSPACE_NOT_FOUND', 'Cannot create a dedicated lockfile for a project that is not in a workspace.')
  }
  await makeDedicatedLockfile(lockfileDir, projectDir)
}
