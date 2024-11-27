#!/bin/sh

if ! command -v git-lfs >/dev/null 2>&1; then
    cat >&2 << EOF
This repository is configured for Git LFS but 'git-lfs' was not found on your path.
Please install git-lfs through: https://github.com/git-lfs/git-lfs#installing
EOF
    exit 2;
fi

git lfs post-checkout "$@"
