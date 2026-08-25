import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const header = '/* DO NOT EDIT - generated from contracts/schemas. */\n'
const rustHeader = '// DO NOT EDIT - generated from contracts/schemas.\n'

const pascal = value => value
  .replace(/[^A-Za-z0-9]+(.)/g, (_, next) => next.toUpperCase())
  .replace(/^[a-z]/, first => first.toUpperCase())
const snake = value => value.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replace(/[^A-Za-z0-9]+/g, '_').toLowerCase()
const identifier = value => {
  const result = pascal(value).replace(/^[^A-Za-z_]/, match => `V${match}`)
  return result || 'Unnamed'
}
const enumIdentifier = value =>
  identifier(/^[A-Z0-9_]+$/.test(value) ? value.toLowerCase() : value)
const rustKeywords = new Set(['type', 'ref', 'self', 'crate', 'match', 'loop', 'move', 'where', 'use', 'mod', 'enum', 'struct'])
const rustField = value => {
  const result = snake(value)
  return rustKeywords.has(result) ? `${result}_` : result
}

export function loadContractSchemas(baseRoot = root) {
  const manifest = JSON.parse(fs.readFileSync(path.join(baseRoot, 'contracts/manifests/schema-manifest.json'), 'utf8'))
  const schemas = new Map()
  for (const entry of manifest.schemas) {
    schemas.set(entry.name, JSON.parse(fs.readFileSync(path.join(baseRoot, 'contracts', entry.path), 'utf8')))
  }
  return { manifest, schemas }
}

function schemaContext(manifest, schemas) {
  const byPath = new Map(manifest.schemas.map(entry => [path.posix.normalize(entry.path), schemas.get(entry.name)]))
  const pathBySchema = new Map(manifest.schemas.map(entry => [schemas.get(entry.name), path.posix.normalize(entry.path)]))
  const resolve = (ref, currentSchema) => {
    const [relative, fragment] = ref.split('#')
    const currentPath = pathBySchema.get(currentSchema)
    const targetPath = relative ? path.posix.normalize(path.posix.join(path.posix.dirname(currentPath), relative)) : currentPath
    const target = byPath.get(targetPath)
    if (!target) throw new Error(`Unknown schema ref ${ref} from ${currentPath}`)
    if (!fragment) return { schema: target, name: identifier(target.title), owner: target }
    const segments = fragment.split('/').filter(Boolean).map(segment => segment.replaceAll('~1', '/').replaceAll('~0', '~'))
    let value = target
    for (const segment of segments) value = value[segment]
    return { schema: value, name: identifier(segments.at(-1)), owner: target }
  }
  return { byPath, pathBySchema, resolve }
}

function tsType(schema, owner, context) {
  if (schema.$ref) return context.resolve(schema.$ref, owner).name
  if (schema.enum) return schema.enum.map(value => JSON.stringify(value)).join(' | ')
  if (Array.isArray(schema.type)) {
    return schema.type.map(type => type === 'null' ? 'null' : tsType({ ...schema, type }, owner, context)).join(' | ')
  }
  if (schema.type === 'string') return 'string'
  if (schema.type === 'integer' || schema.type === 'number') return 'number'
  if (schema.type === 'boolean') return 'boolean'
  if (schema.type === 'array') return `Array<${tsType(schema.items ?? {}, owner, context)}>`
  if (schema.type === 'object' || schema.properties) {
    if (schema.additionalProperties && typeof schema.additionalProperties === 'object') {
      return `Record<string, ${tsType(schema.additionalProperties, owner, context)}>`
    }
    const required = new Set(schema.required ?? [])
    const fields = Object.entries(schema.properties ?? {}).map(([name, child]) =>
      `${JSON.stringify(name)}${required.has(name) ? '' : '?'}: ${tsType(child, owner, context)}`
    )
    return `{ ${fields.join('; ')} }`
  }
  return 'unknown'
}

function renderTsDeclaration(name, schema, owner, context) {
  if (schema.type === 'object' || schema.properties) {
    const required = new Set(schema.required ?? [])
    const fields = Object.entries(schema.properties ?? {}).map(([field, child]) =>
      `  ${JSON.stringify(field)}${required.has(field) ? '' : '?'}: ${tsType(child, owner, context)}`
    )
    return `export interface ${name} {\n${fields.join('\n')}\n}\n`
  }
  return `export type ${name} = ${tsType(schema, owner, context)}\n`
}

function collectRustDeclarations(manifest, schemas, context) {
  const declarations = []
  const names = new Set()
  const inlineNames = new Map()
  const add = (name, schema, owner) => {
    if (names.has(name)) return
    names.add(name)
    declarations.push({ name, schema, owner })
    collectInline(name, schema, owner)
  }
  const collectInline = (parentName, schema, owner) => {
    for (const [field, child] of Object.entries(schema.properties ?? {})) {
      let target = child
      let suffix = identifier(field)
      if (target.type === 'array') {
        target = target.items ?? {}
        suffix += 'Item'
      }
      if (!target.$ref && (target.type === 'object' || target.properties)) {
        const name = `${parentName}${suffix}`
        inlineNames.set(target, name)
        add(name, target, owner)
      }
    }
  }
  for (const entry of manifest.schemas) {
    const schema = schemas.get(entry.name)
    for (const [name, definition] of Object.entries(schema.$defs ?? {})) add(identifier(name), definition, schema)
  }
  for (const entry of manifest.schemas) {
    const schema = schemas.get(entry.name)
    add(identifier(schema.title), schema, schema)
  }
  return { declarations, inlineNames }
}

function rustType(schema, owner, context, inlineNames) {
  if (schema.$ref) return context.resolve(schema.$ref, owner).name
  if (inlineNames.has(schema)) return inlineNames.get(schema)
  if (Array.isArray(schema.type)) {
    const nonNull = schema.type.filter(type => type !== 'null')
    return rustType({ ...schema, type: nonNull[0] }, owner, context, inlineNames)
  }
  if (schema.type === 'string') return 'String'
  if (schema.type === 'integer') return 'i64'
  if (schema.type === 'number') return 'f64'
  if (schema.type === 'boolean') return 'bool'
  if (schema.type === 'array') return `Vec<${rustType(schema.items ?? {}, owner, context, inlineNames)}>`
  if (schema.type === 'object' && schema.additionalProperties && typeof schema.additionalProperties === 'object') {
    return `std::collections::BTreeMap<String, ${rustType(schema.additionalProperties, owner, context, inlineNames)}>`
  }
  return 'serde_json::Value'
}

function renderRustDeclaration({ name, schema, owner }, context, inlineNames) {
  if (schema.enum) {
    const variants = schema.enum.map(value => `    #[serde(rename = ${JSON.stringify(value)})]\n    ${enumIdentifier(String(value))},`)
    return `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]\npub enum ${name} {\n${variants.join('\n')}\n}\n`
  }
  if ((schema.type === 'object' || schema.properties) && !schema.properties && schema.additionalProperties !== false) {
    const valueSchema = typeof schema.additionalProperties === 'object' ? schema.additionalProperties : {}
    const valueType = rustType(valueSchema, owner, context, inlineNames)
    return `pub type ${name} = std::collections::BTreeMap<String, ${valueType}>;\n`
  }
  if (schema.type === 'object' || schema.properties) {
    const required = new Set(schema.required ?? [])
    const fields = Object.entries(schema.properties ?? {}).map(([field, child]) => {
      const nullable = Array.isArray(child.type) && child.type.includes('null')
      const optional = !required.has(field) || nullable
      const base = rustType(child, owner, context, inlineNames)
      const attributes = optional ? '    #[serde(default, skip_serializing_if = "Option::is_none")]\n' : ''
      return `${attributes}    pub ${rustField(field)}: ${optional ? `Option<${base}>` : base},`
    })
    return `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]\n#[serde(rename_all = "camelCase", deny_unknown_fields)]\npub struct ${name} {\n${fields.join('\n')}\n}\n`
  }
  return `pub type ${name} = ${rustType(schema, owner, context, inlineNames)};\n`
}

function dereference(schema, owner, context, stack = new Set()) {
  if (schema.$ref) {
    const resolved = context.resolve(schema.$ref, owner)
    const key = `${context.pathBySchema.get(resolved.owner)}#${resolved.name}`
    if (stack.has(key)) throw new Error(`Recursive schema is not supported in M0A: ${key}`)
    return dereference(resolved.schema, resolved.owner, context, new Set([...stack, key]))
  }
  if (Array.isArray(schema)) return schema.map(value => dereference(value, owner, context, stack))
  if (schema && typeof schema === 'object') {
    return Object.fromEntries(Object.entries(schema).map(([key, value]) => [
      key,
      key === '$id' ? value : dereference(value, owner, context, stack),
    ]))
  }
  return schema
}

function validatorName(title) {
  return `validate${identifier(title)}`
}

export function generateContractSources(manifest, schemas) {
  const context = schemaContext(manifest, schemas)
  const tsDeclarations = []
  const seenTs = new Set()
  for (const entry of manifest.schemas) {
    const schema = schemas.get(entry.name)
    for (const [name, definition] of Object.entries(schema.$defs ?? {})) {
      const id = identifier(name)
      if (!seenTs.has(id)) {
        seenTs.add(id)
        tsDeclarations.push(renderTsDeclaration(id, definition, schema, context))
      }
    }
    const title = identifier(schema.title)
    if (!seenTs.has(title)) {
      seenTs.add(title)
      tsDeclarations.push(renderTsDeclaration(title, schema, schema, context))
    }
  }
  const { declarations, inlineNames } = collectRustDeclarations(manifest, schemas, context)
  const rustDeclarations = declarations.map(item => renderRustDeclaration(item, context, inlineNames))
  return {
    typescript: `${header}\n${tsDeclarations.join('\n')}`,
    rust: `${rustHeader}\nuse serde::{Deserialize, Serialize};\n\n${rustDeclarations.join('\n')}`,
    validators: manifest.schemas.map(entry => {
      const schema = schemas.get(entry.name)
      return { title: identifier(schema.title), schema: dereference(schema, schema, context) }
    }),
  }
}

function writeIfChanged(filePath, content) {
  if (fs.existsSync(filePath) && fs.readFileSync(filePath, 'utf8') === content) return
  fs.writeFileSync(filePath, content)
}

function formatRust(content, filePath) {
  const tempPath = `${filePath}.tmp.rs`
  fs.writeFileSync(tempPath, content)
  try {
    return execFileSync('rustfmt', ['--edition', '2021', '--quiet', '--emit', 'stdout', tempPath], { encoding: 'utf8' })
  } finally {
    fs.rmSync(tempPath, { force: true })
  }
}

export function generateContracts(baseRoot = root) {
  const generatedRoot = process.env.CONTRACTS_GENERATED_DIR ? path.resolve(process.env.CONTRACTS_GENERATED_DIR) : path.join(baseRoot, 'src/generated/contracts')
  const rustGeneratedRoot = process.env.CONTRACTS_RUST_GENERATED_DIR ? path.resolve(process.env.CONTRACTS_RUST_GENERATED_DIR) : path.join(baseRoot, 'src-tauri/src/generated/contracts')
  const { manifest, schemas } = loadContractSchemas(baseRoot)
  const generated = generateContractSources(manifest, schemas)
  fs.mkdirSync(generatedRoot, { recursive: true })
  fs.mkdirSync(rustGeneratedRoot, { recursive: true })
  writeIfChanged(path.join(generatedRoot, 'types.ts'), generated.typescript)
  const schemaEntries = generated.validators.map(({ title, schema }) => `  ${JSON.stringify(title)}: ${JSON.stringify(schema)}`).join(',\n')
  const validatorExports = generated.validators.map(({ title }) =>
    `export const ${validatorName(title)} = (value: unknown): void => validate(${JSON.stringify(title)}, value)`
  ).join('\n')
  writeIfChanged(path.join(generatedRoot, 'validators.ts'), `${header}
import Ajv2020 from 'ajv/dist/2020'
import addFormats from 'ajv-formats'

const schemas = {
${schemaEntries}
} as const
const ajv = new Ajv2020({ allErrors: true, strict: false })
addFormats(ajv)
ajv.addKeyword({
  keyword: 'x-uniqueBy',
  type: 'array',
  schemaType: 'string',
  validate: (property: string, data: Array<Record<string, unknown>>) => {
    const values = data.map(item => item?.[property])
    return values.length === new Set(values).size
  },
})
const compiled = Object.fromEntries(Object.entries(schemas).map(([name, schema]) => [name, ajv.compile(schema)]))
function validate(name: keyof typeof schemas, value: unknown): void {
  const validator = compiled[name]
  if (!validator(value)) throw new Error('SCHEMA_INVALID:' + name + ':' + ajv.errorsText(validator.errors))
}
${validatorExports}
`)
  writeIfChanged(path.join(generatedRoot, 'schemaVersions.ts'), `${header}
export const CONTRACT_VERSION = 'm0a.v1' as const
export const ARTIFACT_SCHEMA_VERSION = 'm0a.artifact.v1' as const
export const SCHEMA_MANIFEST_VERSION = ${JSON.stringify(manifest.manifestVersion)} as const
`)
  writeIfChanged(path.join(generatedRoot, 'index.ts'), `${header}
export * from './types'
export * from './validators'
export * from './schemaVersions'
`)
  const rustTypesPath = path.join(rustGeneratedRoot, 'types.rs')
  writeIfChanged(rustTypesPath, formatRust(generated.rust, rustTypesPath))
  const rustValidators = generated.validators.map(({ title, schema }) => {
    const constName = snake(title).toUpperCase()
    const body = title === 'CommitBundle'
      ? `validate_schema(${constName}, value)?;
    let mut ids = std::collections::HashSet::new();
    for artifact in value.get("artifacts").and_then(serde_json::Value::as_array).into_iter().flatten() {
        let id = artifact.get("artifactId").and_then(serde_json::Value::as_str).ok_or_else(|| "artifactId missing".to_string())?;
        if !ids.insert(id) { return Err("duplicate artifactId".to_string()); }
    }
    Ok(())`
      : `validate_schema(${constName}, value)`
    return `const ${constName}: &str = r###"${JSON.stringify(schema)}"###;
pub fn validate_${snake(title)}(value: &serde_json::Value) -> Result<(), String> {
    ${body}
}`
  }).join('\n\n')
  const rustValidatorsPath = path.join(rustGeneratedRoot, 'validators.rs')
  writeIfChanged(rustValidatorsPath, formatRust(`${rustHeader}
fn validate_schema(schema: &str, value: &serde_json::Value) -> Result<(), String> {
    let schema: serde_json::Value = serde_json::from_str(schema).map_err(|error| error.to_string())?;
    jsonschema::draft202012::validate(&schema, value).map_err(|error| error.to_string())
}

pub fn validate_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

${rustValidators}
`, rustValidatorsPath))
  writeIfChanged(path.join(rustGeneratedRoot, 'mod.rs'), `${rustHeader}pub mod schema_versions;\npub mod types;\npub mod validators;\npub use types::*;\n`)
  const rustVersionsPath = path.join(rustGeneratedRoot, 'schema_versions.rs')
  writeIfChanged(rustVersionsPath, formatRust(`${rustHeader}pub const CONTRACT_VERSION: &str = "m0a.v1";\npub const ARTIFACT_SCHEMA_VERSION: &str = "m0a.artifact.v1";\npub const SCHEMA_MANIFEST_VERSION: &str = ${JSON.stringify(manifest.manifestVersion)};\n`, rustVersionsPath))
  console.log('generated contracts')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) generateContracts()
