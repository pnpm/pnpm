'use strict'
const { getCurrentBranch } = require('@pnpm-utils').default

main()
async function main() {
  const branchName = await getCurrentBranch();
  console.log(branchName)
}
