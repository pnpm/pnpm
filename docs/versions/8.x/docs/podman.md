---
id: podman
title: Working with Podman
---

## Sharing Files Between a Container and the Host Btrfs Filesystem

:::note

This method only works on copy-on-write filesystems supported by Podman, such as Btrfs. For other filesystems, like Ext4, pnpm will copy the files instead.

:::

Podman support copy-on-write filesystems like Btrfs. With Btrfs, container runtimes create actual Btrfs subvolumes for their mounted volumes. pnpm can leverage this behavior to reflink the files between different mounted volumes.

To share files between the host and the container, mount the store directory and the `node_modules` directory from the host to the container. This allows pnpm inside the container to naturally reuse the files from the host as reflinks.

Below is an example container setup for demonstration:

```dockerfile title="Dockerfile"
FROM node:20-slim

# corepack is an experimental feature in Node.js v20 which allows
# installing and managing versions of pnpm, npm, yarn
RUN corepack enable

VOLUME [ "/pnpm-store", "/app/node_modules" ]
RUN pnpm config --global set store-dir /pnpm-store

# You may need to copy more files than just package.json in your code
COPY package.json /app/package.json

WORKDIR /app
RUN pnpm install
RUN pnpm run build
```

Run the following command to build the podman image:

```sh
podman build . --tag my-podman-image:latest -v "$HOME/.local/share/pnpm/store:/pnpm-store" -v "$(pwd)/node_modules:/app/node_modules"
```
