import test from 'ava'

import { share } from '../index'
import { spawnSync } from 'node:child_process'
import { dirname } from 'path'
import { fileURLToPath } from 'url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

test('share across child process', (t) => {
  const payload = Buffer.from('hello world')
  const pipe = share(payload)
  console.log('pipe', pipe, pipe.fd, pipe.name)
  const child = spawnSync(process.execPath, [`${__dirname}/child.js`], {
    env: { ...process.env, CONFIG_PATH: '/proc/self/fd/3' },
    stdio: ['ignore', 'pipe', 'ignore', pipe.fd],
    encoding: 'utf8',
  })

  // console.log("STDOUT:", child.stdout);
  // console.log("STDERR:", child.stderr);
  // console.log("EXIT CODE:", child.status);
  // console.log("ERROR:", child.error);

  // let output = '';
  // child.stdout.on('data', (data) => { output += data.toString(); });
  //
  // await new Promise((resolve) => child.on('close', resolve));

  pipe.close()

  t.is(child.stdout.trim(), 'hello world')
})
