#!/usr/bin/env node
import { lockfileToPnp } from '.'

lockfileToPnp(process.cwd())
  .then(() => console.log('Created .pnp.js'))
  .catch((err) => console.error(err))
