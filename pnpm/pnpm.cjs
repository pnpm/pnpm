// CJS → ESM bridge for pkg/SEA
;(async () => {
  // import the real ESM entry; side effects will run like a CLI
  await import('./dist/pnpm.mjs')
})()
