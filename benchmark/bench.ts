import { Bench } from 'tinybench'

import { shareMemFD, shareSHM, shareNamedPipe, share } from '../index.js'

const b = new Bench()

const buffer = Buffer.from(JSON.stringify({ a: 1, b: 2, c: 3 }))

if (process.platform === 'linux' || process.platform === 'freebsd' || process.platform === 'darwin') {
  if (process.platform !== 'darwin') {
    b.add('shareMemFD', () => {
      shareMemFD(buffer).close()
    })
  }
  b.add('shareSHM', () => {
    shareSHM(buffer).close()
  })
}

if (process.platform === 'win32') {
  b.add('shareNamedPipe', () => {
    shareNamedPipe(buffer).close()
  })
}

b.add('share', () => {
  share(buffer).close()
})

await b.run()

console.table(b.table())
