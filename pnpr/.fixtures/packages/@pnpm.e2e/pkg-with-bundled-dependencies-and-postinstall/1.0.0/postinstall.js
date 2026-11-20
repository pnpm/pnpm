const fs = require('fs')

// Lets a test prove that an install restored the build from the side-effects cache.
if (process.env.PNPM_E2E_FAIL_POSTINSTALL) {
  console.error('postinstall ran although the build was expected to come from the side-effects cache')
  process.exit(1)
}

fs.writeFileSync('generated-by-postinstall.js', '')
