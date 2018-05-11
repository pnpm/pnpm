#!/bin/bash
set -e
set -u

if [[ $TRAVIS_PULL_REQUEST_SLUG != "" && $TRAVIS_PULL_REQUEST_SLUG != $TRAVIS_REPO_SLUG ]]; then
    # This is a Pull Request from a different slug, hence a forked repository
    git remote add "$TRAVIS_PULL_REQUEST_SLUG" "https://github.com/$TRAVIS_PULL_REQUEST_SLUG.git"
    git fetch "$TRAVIS_PULL_REQUEST_SLUG"

    # Use the fetched remote pointing to the source clone for comparison
    TO="$TRAVIS_PULL_REQUEST_SLUG/$TRAVIS_PULL_REQUEST_BRANCH"
else
    # This is a Pull Request from the same remote, no clone repository
    TO=$TRAVIS_COMMIT
fi

# Lint all commits in the PR
# - Covers fork pull requests (when TO=slug/branch)
# - Covers branch pull requests (when TO=branch)
./node_modules/.bin/commitlint --from="$TRAVIS_BRANCH" --to="$TO"

# Always lint the triggerig commit
# - Covers direct commits
./node_modules/.bin/commitlint --from="$TRAVIS_COMMIT"
