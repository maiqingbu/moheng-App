import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve('src')
const projectRoot = path.dirname(root)
const rustRoot = path.join(projectRoot, 'src-tauri/src')
const rustFiles = []
const walk = dir => {
  for (const name of fs.readdirSync(dir)) {
    const file = path.join(dir, name)
    if (fs.statSync(file).isDirectory()) walk(file)
    else if (name.endsWith('.rs')) rustFiles.push(file)
  }
}
walk(rustRoot)
const productionRust = rustFiles.filter(file => !file.includes(`${path.sep}tests${path.sep}`)).map(file => fs.readFileSync(file, 'utf8')).join('\n')
const frontendFiles = []
const walkFrontend = dir => {
  for (const name of fs.readdirSync(dir)) {
    const file = path.join(dir, name)
    if (fs.statSync(file).isDirectory()) walkFrontend(file)
    else if (/\.(ts|vue)$/.test(name)) frontendFiles.push(file)
  }
}
walkFrontend(root)
const frontend = frontendFiles.map(file => fs.readFileSync(file, 'utf8')).join('\n')
const narrativeRuntime = fs.readFileSync(path.join(rustRoot, 'narrative/runtime.rs'), 'utf8')
const harness = fs.readFileSync(path.join(rustRoot, 'harness/mod.rs'), 'utf8')
const count = (text, pattern) => [...text.matchAll(pattern)].length
const legacyPlannerCount = count(`${productionRust}\n${frontend}`, /\bLegacyPlanner\b/g)
const narrativeRuntimeDeltaCount = count(`${productionRust}\n${frontend}`, /\bNarrativeRuntimeDelta\b/g)
const commitBundleV3Count = count(`${productionRust}\n${frontend}`, /CommitBundle\s+V3/g)
const secondAuthorityCount = count(productionRust, /CREATE\s+TABLE(?:\s+IF\s+NOT\s+EXISTS)?\s+(?:narrative_state|continuity_threads|memory_store|timeline_events_v2|future_reservations|volume_task_graph|rolling_horizon)\b/ig)
const tsAuthoritativeCompilerCount = count(frontend, /(?:compileContextManifest|NarrativeCompiler|relevantStateDigest)\s*\(/g)
const narrativeRawSqlCount = count(narrativeRuntime, /(?:\bSELECT\s+|\bINSERT\s+INTO|\bUPDATE\s+\w+|\bDELETE\s+FROM)/ig)
const harnessRawSqlCount = count(harness, /(?:\bSELECT\s+|\bINSERT\s+INTO|\bUPDATE\s+\w+|\bDELETE\s+FROM)/ig)
const h01WriteCapabilityCount = count(harness, /(?:CommitBundle|execute_command|write_authority|EXECUTE_APPROVED_REPAIR|arbitrary_sql_write)/g)
const skillRuntimeIntegrationCount = count(`${productionRust}\n${frontend}`, /(?:SkillProfile|SkillDispatcher|skill_runtime|register_skill|creative[-_ ]skill)/gi)
const b07SymbolCount = count(`${productionRust}\n${frontend}`, /(?:VolumeIntentGenerator|RollingHorizon|ChapterPlanGenerator)/g)
const b08SymbolCount = count(`${productionRust}\n${frontend}`, /(?:FinalChapterRenderer|ChapterCandidateProducer)/g)
const values = { legacyPlannerCount, narrativeRuntimeDeltaCount, commitBundleV3Count, secondAuthorityCount, tsAuthoritativeCompilerCount, narrativeRawSqlCount, harnessRawSqlCount, h01WriteCapabilityCount, skillRuntimeIntegrationCount, b07SymbolCount, b08SymbolCount }
const bad = Object.entries(values).filter(([, value]) => value !== 0)
if (bad.length) {
  console.error(`B06_H01_ARCHITECTURE_FAIL ${bad.map(([key, value]) => `${key}=${value}`).join(' ')}`)
  process.exit(1)
}
console.log(`B06_H01_ARCHITECTURE_RESULT ${Object.entries(values).map(([key, value]) => `${key}=${value}`).join(' ')} status=PASS`)
