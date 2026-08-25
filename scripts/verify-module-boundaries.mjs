import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

// Module-boundary verifier for M0-01 §7 import guard.
// Layers are derived from the first segment under src/.
const SRC = path.resolve('src')

// Allowed direction matrix. Key = importer layer, value = set of layers it may import.
// Absent layers are implicitly disallowed (deny-by-default for the 4 core business layers).
const ALLOWED = {
  ui: new Set(['ui', 'application', 'domain', 'app']),
  application: new Set(['application', 'domain', 'services', 'generated']),
  domain: new Set(['domain', 'generated']),
  services: new Set(['services', 'domain', 'application', 'generated']),
  production: new Set(['production', 'application', 'domain', 'services', 'generated']),
  // infrastructure can reference common/core contracts; browser-demo is marked separately below.
  infrastructure: new Set(['infrastructure', 'generated', 'application', 'domain']),
  // app is the composition root / router shell; allowed to wire most layers.
  app: new Set(['app', 'ui', 'application', 'domain', 'services', 'production', 'infrastructure', 'generated']),
  generated: new Set(['generated']),
}

const FORBIDDEN_RULES = [
  // Rule F: browser-demo must never be imported by the production/tauri path.
  {
    name: 'browser-demo must not be imported by tauri adapter or production',
    test: (from, target) => {
      const fromIsTauri = from.includes(path.join('infrastructure', 'tauri'))
      const fromIsProduction = from.startsWith(path.join('production', ''))
      const targetIsDemo = target && target.includes(path.join('infrastructure', 'browser-demo'))
      return (fromIsTauri || fromIsProduction) && targetIsDemo
    },
  },
]

const FORBIDDEN_SOURCE_RULES = [
  {
    name: 'UI/application must not call Tauri or SQLite APIs directly',
    test: (relative, content) => {
      const layer = relative.split(path.sep)[0]
      const forbiddenImport = /^(?:import|export).*?(?:from\s*)?["'](?:[^"']*tauri|[^"']*sqlite|[^"']*rusqlite)/im.test(content)
      const forbiddenApiCall = /\b(?:invoke|listen)\s*\(/.test(content)
      return ['ui', 'application'].includes(layer) && (forbiddenImport || forbiddenApiCall)
    },
  },
]

// Walk all source files under src/.
const files = []
const walk = (dir) => {
  for (const name of fs.readdirSync(dir)) {
    const full = path.join(dir, name)
    if (name === '.gitkeep') continue
    if (fs.statSync(full).isDirectory()) walk(full)
    else if (/\.(ts|vue|js|mjs)$/.test(name)) files.push(full)
  }
}
walk(SRC)

const errors = []

const normalize = (p) => path.normalize(p).split(path.sep).join(path.sep)

// Determine the layer of a src-relative path. Files directly under src/ (e.g.
// App.vue, main.ts) are the app/bootstrap layer; otherwise the first directory
// segment is the layer.
const layerOfRel = (srcRel) => {
  const seg = normalize(srcRel).split(path.sep)
  if (seg.length === 1) return 'app' // top-level src files (App.vue, main.ts) are the bootstrap layer
  return seg[0]
}

const layerOf = layerOfRel

const targetLayer = (absPath) => {
  const rel = path.relative(SRC, absPath)
  if (rel.startsWith('..') || path.isAbsolute(rel)) return null // outside src/
  return layerOfRel(rel)
}

const toSrcRel = (absPath) => path.relative(SRC, absPath)

const importedPathsFrom = (content) => {
  const out = []
  // import ... from 'x'  /  export ... from 'x'  /  import 'x'
  const re = /(?:^|[\n;])\s*(?:import|export)\s[^'"]*?from\s*['"]([^'"]+)['"]|(?:^|[\n;])\s*import\s*['"]([^'"]+)['"]|import\s*\(\s*['"]([^'"]+)['"]/g
  let m
  while ((m = re.exec(content)) !== null) {
    out.push(m[1] || m[2] || m[3])
  }
  return out
}

for (const file of files) {
  const srcRel = toSrcRel(file)
  const fromLayer = layerOf(srcRel)
  const content = fs.readFileSync(file, 'utf8')
  for (const rule of FORBIDDEN_SOURCE_RULES) {
    if (rule.test(srcRel, content)) {
      errors.push(`[source] ${rule.name}: ${srcRel}`)
    }
  }
  const specifiers = importedPathsFrom(content)

  for (const spec of specifiers) {
    // external packages (no ./ or ../ or leading path segment that maps inside src)
    if (spec.startsWith('.') === false && spec.startsWith('/') === false) {
      // could be absolute alias or node package; treat non-relative as external, ignore.
      continue
    }
    let resolved
    if (spec.startsWith('.')) {
      resolved = path.resolve(path.dirname(file), spec)
    } else if (spec.startsWith('/')) {
      resolved = path.join(SRC, path.sep, spec.replace(/^\/+/, '')) // treat as src-rooted
    } else {
      continue
    }
    const tLayer = targetLayer(resolved)
    if (tLayer === null) continue // not inside src (e.g. resolves outside)

    // Forbidden special rules
    for (const rule of FORBIDDEN_RULES) {
      if (rule.test(toSrcRel(file), toSrcRel(resolved))) {
        errors.push(`[boundary] ${rule.name}
  ${srcRel} -> ${toSrcRel(resolved)}`)
      }
    }

    // Deny-by-default check against the allowed matrix.
    const allowed = ALLOWED[fromLayer]
    if (allowed && !allowed.has(tLayer) && fromLayer !== tLayer) {
      errors.push(`[import] ${fromLayer} -> ${tLayer} not allowed (${srcRel} -> ${toSrcRel(resolved)})`)
    } else if (!allowed && fromLayer && fromLayer !== tLayer) {
      errors.push(`[import] unknown layer '${fromLayer}' (${srcRel} -> ${toSrcRel(resolved)})`)
    }
  }
}

// Rule G: generated/contracts files must carry a generated marker and never be hand-edited.
const generatedDir = path.join(SRC, 'generated')
if (fs.existsSync(generatedDir)) {
  const walkGen = (dir) => {
    for (const name of fs.readdirSync(dir)) {
      const full = path.join(dir, name)
      if (fs.statSync(full).isDirectory()) walkGen(full)
      else if (/\.(ts|rs)$/.test(name)) {
        const c = fs.readFileSync(full, 'utf8').slice(0, 400)
        if (!/generated|DO NOT EDIT|auto[- ]?generated/i.test(c)) {
          errors.push(`[generated] ${toSrcRel(full)} missing generated marker`)
        }
      }
    }
  }
  walkGen(generatedDir)
}

if (errors.length) {
  console.error('Module boundary verification failed:\n- ' + errors.join('\n- '))
  process.exit(1)
}
console.log(`Module boundary verification passed: ${files.length} files scanned.`)