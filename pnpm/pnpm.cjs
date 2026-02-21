// CJS → ESM bridge for Node.js SEA (Single Executable Application)
;(async () => {
  // import the real ESM entry; side effects will run like a CLI
  await import('./dist/pnpm.mjs')
})()
