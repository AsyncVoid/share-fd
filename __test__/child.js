import fs from 'node:fs'
const test = fs.readFileSync(process.env.CONFIG_PATH, 'utf-8')
console.log(test)
