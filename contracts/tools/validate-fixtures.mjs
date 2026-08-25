import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import Ajv2020 from 'ajv/dist/2020.js'
import addFormats from 'ajv-formats'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const manifest = JSON.parse(fs.readFileSync(path.join(root, 'contracts/manifests/schema-manifest.json'), 'utf8'))
const schemas = manifest.schemas.map(entry => ({
  name: entry.name,
  schema: JSON.parse(fs.readFileSync(path.join(root, 'contracts', entry.path), 'utf8')),
}))
const ajv = new Ajv2020({ allErrors: true, strict: false })
addFormats(ajv)
ajv.addKeyword({
  keyword: 'x-uniqueBy',
  type: 'array',
  schemaType: 'string',
  validate: (property, data) => {
    const values = data.map(item => item?.[property])
    return values.length === new Set(values).size
  },
})
for (const { schema } of schemas) ajv.addSchema(schema)
const validators = schemas
  .filter(({ schema }) => schema.type || schema.properties)
  .map(({ name, schema }) => ({ name, validate: ajv.getSchema(schema.$id) ?? ajv.compile(schema) }))

function matchingSchemas(value) {
  return validators.filter(({ validate }) => validate(value)).map(({ name }) => name)
}

function fixtureFiles(kind) {
  const directory = path.join(root, 'contracts/fixtures', kind)
  return fs.readdirSync(directory).filter(name => name.endsWith('.json')).map(name => path.join(directory, name))
}

const failures = []
for (const file of fixtureFiles('valid')) {
  const value = JSON.parse(fs.readFileSync(file, 'utf8'))
  if (matchingSchemas(value).length === 0) failures.push(`valid fixture rejected: ${path.basename(file)}`)
}
for (const file of fixtureFiles('invalid')) {
  const value = JSON.parse(fs.readFileSync(file, 'utf8'))
  const matches = matchingSchemas(value)
  if (matches.length > 0) failures.push(`invalid fixture accepted by ${matches.join(',')}: ${path.basename(file)}`)
}
if (failures.length) {
  console.error(`Contract validation failed:\n- ${failures.join('\n- ')}`)
  process.exit(1)
}
console.log(`Contract fixtures validated through Draft 2020-12 schemas: ${fixtureFiles('valid').length} valid accepted, ${fixtureFiles('invalid').length} invalid rejected.`)
