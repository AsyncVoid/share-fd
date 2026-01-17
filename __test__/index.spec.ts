import test from 'ava'

import { Pipe, NamedPipe, share } from '../index'
import { spawnSync } from 'node:child_process'
import { dirname } from 'path'
import { fileURLToPath } from 'url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

function isPipe(pipe: Pipe | NamedPipe): pipe is Pipe {
  return pipe instanceof Pipe || pipe.name !== undefined
}

function isNamedPipe(pipe: Pipe | NamedPipe): pipe is NamedPipe {
  return pipe instanceof NamedPipe || pipe.path !== undefined
}

test('share across child process', async (t) => {
  const payload = Buffer.from('hello world')
  const pipe = await share(payload);

  let child

  if (isPipe(pipe)) {
    console.log('pipe', pipe, pipe.fd, pipe.name)
    child = spawnSync(process.execPath, [`${__dirname}/child.js`], {
      env: { ...process.env, CONFIG_PATH: '/proc/self/fd/3' },
      stdio: ['ignore', 'pipe', 'ignore', pipe.fd],
      encoding: 'utf8',
    })
  } else if (isNamedPipe(pipe)) {
    console.log('pipe', pipe, pipe.path)
    child = spawnSync(process.execPath, [`${__dirname}/child.js`], {
      env: { ...process.env, CONFIG_PATH: pipe.path },
      stdio: ['ignore', 'pipe', 'ignore'],
      encoding: 'utf8',
    })
  } else {
    t.fail('result is not a pipe or named pipe')
  }

  console.log('STDOUT:', child.stdout)
  console.log('STDERR:', child.stderr)
  console.log('EXIT CODE:', child.status)
  console.log('ERROR:', child.error)

  pipe.close()

  t.is(child.stdout.trim(), 'hello world')
})
