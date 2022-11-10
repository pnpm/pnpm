#! /usr/bin/env bash

# old version /home/user/src/javascript/nodejs-lockfile-parser/nodejs-lockfile-parser/test/validate-lockfiles-pnpm.sh

set -e # exit on error

#set -x # trace

(
  # nodejs-lockfile-parser
  #find /home/user/src/javascript/nodejs-lockfile-parser/nodejs-lockfile-parser/test/fixtures/ -name 'pnpm-lock.v*.yaml' -not -path '*/node_modules/*' | shuf | while read -r lockfilePath

  # lockfiles scraped from github
  find /home/user/src/javascript/github-scrape-files/github/ -name 'pnpm-lock.yaml' | shuf | while read -r lockfilePath
  do
    node /home/user/src/javascript/pnpm/git/pnpm/packages/pnpm/test/install/lockfile.todo.mjs "$lockfilePath"
  done
)
