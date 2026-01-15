# `@asyncvoid/share-fd`

![CI](https://github.com/AsyncVoid/share-fd/workflows/CI/badge.svg)

> Cross-platform inter-process data sharing using file descriptors and named pipes.

## Overview

`@asyncvoid/share-fd` provides a unified API for sharing data between processes using platform-specific memory sharing mechanisms. Data is written to a file descriptor or named pipe that can be passed to child processes, allowing efficient zero-copy data transfer disguised as a file path.

### Platform Support

- **Linux/FreeBSD/NetBSD**: `memfd_create` (anonymous memory-backed file descriptors)
- **macOS/Unix**: `shm_open` (POSIX shared memory)
- **Windows**: Named Pipes

The `share()` function automatically selects the optimal method for your platform.

## Installation

```bash
npm install @asyncvoid/share-fd
# or
yarn add @asyncvoid/share-fd
```

## Usage

### Basic Example

```javascript
import { share } from '@asyncvoid/share-fd'
import { spawnSync } from 'node:child_process'

// Create a pipe with your data
const payload = Buffer.from('hello world')
const pipe = share(payload)

// Pass the file descriptor to a child process
const child = spawnSync(process.execPath, ['./child.js'], {
  env: { ...process.env, CONFIG_PATH: '/proc/self/fd/3' },
  stdio: ['ignore', 'pipe', 'ignore', pipe.fd],
  encoding: 'utf8',
})

console.log(child.stdout) // "hello world"

// Clean up when done
pipe.close()
```

### Child Process (child.js)

```javascript
import fs from 'node:fs'

// Read from the file descriptor passed via environment variable
const data = fs.readFileSync(process.env.CONFIG_PATH, 'utf-8')
console.log(data)
```

### Platform-Specific APIs

You can also use platform-specific functions directly:

```javascript
import { shareMemFD, shareSHM, shareNamedPipe } from '@asyncvoid/share-fd'

// Linux/FreeBSD only - memfd_create
if (process.platform === 'linux') {
  const pipe = shareMemFD(Buffer.from('data'))
  console.log(pipe.fd, pipe.name)
  pipe.close()
}

// Unix/macOS - shm_open
if (process.platform === 'darwin') {
  const pipe = shareSHM(Buffer.from('data'))
  console.log(pipe.fd, pipe.name)
  pipe.close()
}

// Windows - Named Pipes
if (process.platform === 'win32') {
  const pipe = shareNamedPipe(Buffer.from('data'))
  console.log(pipe.path)
  pipe.close()
}
```

## API

### `share(payload: Buffer, name?: string): Pipe`

Creates a platform-appropriate shared memory region with the given payload. Automatically selects:
- `memfd_create` on Linux/FreeBSD/NetBSD
- `shm_open` on macOS and other Unix systems
- Named Pipes on Windows

**Parameters:**
- `payload`: Buffer containing the data to share
- `name` (optional): Name hint for the shared resource

**Returns:** `Pipe` object with platform-specific properties

### `shareMemFD(payload: Buffer, name?: string): Pipe`

Linux/FreeBSD/NetBSD only. Creates an anonymous memory-backed file descriptor using `memfd_create`.

### `shareSHM(payload: Buffer, name?: string): Pipe`

Unix/macOS. Creates a POSIX shared memory object using `shm_open`.

### `shareNamedPipe(payload: Buffer, name?: string): Pipe`

Windows only. Creates a named pipe for inter-process communication.

### Pipe Object

**Unix/Linux (`shareMemFD`, `shareSHM`):**
- `fd: number` - File descriptor that can be passed to child processes
- `name: string` - Name of the shared resource
- `close(): void` - Closes the file descriptor

**Windows (`shareNamedPipe`):**
- `path: string` - Named pipe path (e.g., `\\.\pipe\share-<ulid>`)
- `close(): void` - Signals the pipe to stop accepting connections

### Low-Level APIs

```javascript
import { memfd_create, shm_open, set_cloexec } from '@asyncvoid/share-fd'

// Create a memfd (Linux only)
const fd = memfd_create('myname', 0)

// Create shared memory (Unix)
const fd = shm_open('myshm', O_CREAT | O_RDWR, 0o600)

// Control close-on-exec flag (Unix)
set_cloexec(fd, true)
```

## Development

### Requirements

- Rust (latest stable)
- Node.js 12.22.0+ / 14.17.0+ / 15.12.0+ / 16.0.0+
- yarn 1.x

### Build

```bash
yarn install
yarn build
```

This will generate platform-specific native binaries (`.node` files).

### Test

```bash
yarn test
```

Runs the test suite using [ava](https://github.com/avajs/ava).

### Benchmark

```bash
yarn bench
```

Runs performance benchmarks comparing different sharing methods.

## CI/CD

With GitHub Actions, each commit and pull request is automatically built and tested across:
- **Node versions**: 20, 22
- **Platforms**: Linux (x64, ARM64), macOS (x64, ARM64), Windows (x64, ARM64)
- **Variants**: GNU libc, musl libc

Pre-built binaries are published as platform-specific optional dependencies, ensuring users don't need a Rust toolchain installed.

## How It Works

This package leverages platform-specific APIs to create memory regions that can be accessed as file paths:

1. **Data is written** to a file descriptor or named pipe
2. **The descriptor is passed** to a child process via `stdio` array
3. **Child process reads** from the descriptor using standard file I/O
4. **No disk I/O** occurs - data stays in memory

This is particularly useful for:
- Passing configuration to child processes
- Sharing large data structures without serialization overhead
- Avoiding command-line argument length limits
- Secure data transfer (no temporary files on disk)

## License

MIT
