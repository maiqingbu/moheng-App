# R2 Batch Freeze Through B09/H04

本文件是 B10-T00 开工前冻结记录。它只记录已经正式接受的前序批次与本批允许的重新打开边界，不重新评审历史设计。

## Frozen baseline

- Architecture: `墨衡 V2.1 Architecture R2 FROZEN`
- PRD: `墨衡 V2.1 PRD R2 FROZEN`
- Freeze through: `B09_H04`
- Accepted baseline: `2a5f741527eaf62830141f80d88dcbf3ecc69017`
- Next authorized batch: `B10`

## Accepted stages

`M0/B01–B04`, `B05`, `B06/H01`, `B07/H02`, `B08/H03`, `Post-B08/H03 R2 Alignment`, `P01`, and `B09/H04` remain FINAL PASS / FROZEN.

Known accepted source chain:

- B08/H03 pre-alignment: `ec1a12176100ba427395c2d62bdc1661dcabfa5e`
- R2 Alignment: `bc1878bce9613a7ffb184227531372177d72bdeb`
- P01: `6992b544f59f385160e3aa2f34f2e206ae715e59`
- B09/H04: `2a5f741527eaf62830141f80d88dcbf3ecc69017`

## Reopen policy

Only B10-authorized dependency integration, a B10 regression, a new R2 hard-invariant violation, or a separate user instruction may reopen a frozen surface. Reviewer preference, style, a new test organization, and opportunistic refactoring are not reopen reasons.

Migrations `0001` through `0010` are byte-frozen. B10 may add only an append-only `0011+` migration when a typed SkillInvocation or Quality persistence contract actually requires it; Skill and Quality remain non-authoritative for Narrative truth.

## B10-T00 Git gate

- HEAD before B10 functional changes: `2a5f741527eaf62830141f80d88dcbf3ecc69017`
- Working tree before B10 functional changes: `CLEAN`
- Next authorized batch: `B10`
