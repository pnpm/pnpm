'use strict'
const createResolveFromNpm = require('@pnpm/git-resolver').default

const resolveFromNpm = createResolveFromNpm({})

resolveFromNpm({
  pref: 'kevva/is-negative#16fd36fe96106175d02d066171c44e2ff83bc055'
})
.then(resolveResult => console.log(JSON.stringify(resolveResult, null, 2)))
