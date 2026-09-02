import path from 'node:path'

export function packageGeneratorEnvironment(packageRoot) {
  return {
    B12_GENERATOR_SELF_CHECK: '1',
    B12_EVIDENCE_DIR: path.join(packageRoot, 'evidence/B12'),
  }
}
