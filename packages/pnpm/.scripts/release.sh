#!/bin/bash
set -e
set -u

pnpm run compile
npm cache clear --force
publish-packed --tag next --prune
