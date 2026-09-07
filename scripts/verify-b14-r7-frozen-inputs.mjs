import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

const inputDir = path.resolve(process.env.B14_R7_INPUT_DIR ?? '/Users/mac/Downloads')
const files = [
  ['B14_ORIGINAL', '墨衡_V2.1_B14_EndgameCompletion_终局与完结治理_开发验收与打包执行指令_R1_2026-09-05.md', 'e67cba88e4fbe9c898e22f6215fa7a8dea6f3206bf73ba417ff3a5abd6ede6e0'],
  ['B14_R1', '墨衡_V2.1_B14_R1_定点返工与重新打包执行指令_2026-09-06.md', '907963285af25ac22a330f4c68206c93d3417e456e131638d8e73fe024e0f508'],
  ['B14_R2', '墨衡_V2.1_B14_R2_最终集成返工与重新打包执行指令_2026-09-06.md', '6c3a3e7306af0f667760ea703b2a288a89f2577a4467e9b45bcd52a869618573'],
  ['B14_R3', '墨衡_V2.1_B14_R3_最终封口返工与正式重新打包执行指令_2026-09-06.md', '87296439a034eadc6548a0c81cf5ea8b2f646e189adc3f8fbc0e008409f190d9'],
  ['B14_R4', '墨衡_V2.1_B14_R4_最终微补丁与正式重新打包执行指令_2026-09-07.md', 'c18bd1fa2c141442b5c90637374b671fd1f61a91ed0b85d6509271162accf6ae'],
  ['B14_R5', '墨衡_V2.1_B14_R5_两个集成漏接与Evidence真实性最终封口执行指令_2026-09-07.md', 'a7de5640ee1d8651f0a5db37a12a0d37463287adb6ad41b2c60ae6c4c8e3e446'],
  ['B14_R6', '墨衡_V2.1_B14_R6_EvidenceOnly最终重打包执行指令_2026-09-07.md', '50a21d6ba95999befd1ce02d7ad1850b36b4a310cce4ad1721d1e319db593414'],
  ['B14_R7', '墨衡_V2.1_B14_R7_验收定义修正与最终认证执行指令_2026-09-07.md', 'd34df5cb7b4b0e93a51c966e50536d3ee3c30b14f99b3039808f4175fa974934'],
  ['FREEZE_THROUGH_B13', '墨衡_V2.1_R2_批次冻结记录_Freeze_Through_B13_2026-09-05.md', 'd198723eae4441c9d538155b18cddd938611285ce23ae873ecde2790c9d18933'],
]
const documents = files.map(([documentRole, filename, expectedSha256]) => {
  const file = path.join(inputDir, filename)
  const actualSha256 = fs.existsSync(file) ? crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') : null
  return { filename, documentRole, expectedSha256, actualSha256, exactMatch: actualSha256 === expectedSha256 }
})
const mismatch = documents.find(item => !item.exactMatch)
const result = { schema: 'b14-r7-frozen-inputs', inputDir, documents, failureClass: mismatch ? 'FROZEN_INPUT_SHA_MISMATCH' : null, status: mismatch ? 'FAIL' : 'PASS' }
console.log(JSON.stringify(result, null, 2))
if (result.status !== 'PASS') process.exitCode = 1
