---
id: server
title: pnpm server
---

:::danger

Deprecated feature

:::

Manage a store server.

## Commands

### pnpm server start

Starts a server that performs all interactions with the store.
Other commands will delegate any store-related tasks to this server.

### pnpm server stop

Stops the store server.

### pnpm server status

Prints information about the running server.

## Options

### --background

* Default: **false**
* Type: **Boolean**

Runs the server in the background, similar to daemonizing on UNIX systems.

### --network-concurrency

* Default: **null**
* Type: **Number**

The maximum number of network requests to process simultaneously.

### --protocol

* Default: **auto**
* Type: **auto**, **tcp**, **ipc**

The communication protocol used by the server.
When this is set to `auto`, IPC is used on all systems except for Windows,
which uses TCP.

### --port

* Default: **5813**
* Type: **port number**

The port number to use when TCP is used for communication.
If a port is specified and the protocol is set to `auto`, regardless of system
type, the protocol is automatically set to use TCP.

### --store-dir

* Default: **&lt;home\>/.pnpm-store**
* Type: **Path**

The directory to use for the content addressable store.

### --[no-]lock

* Default: **false**
* Type: **Boolean**

Set whether to make the package store immutable to external processes while
the server is running or not.

### --ignore-stop-requests

* Default: **false**
* Type: **Boolean**

Prevents you from stopping the server using `pnpm server stop`.

### --ignore-upload-requests

* Default: **false**
* Type: **Boolean**

Prevents creating a new side effect cache during install.
