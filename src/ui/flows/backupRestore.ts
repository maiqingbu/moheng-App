// FX-22: restoring a work from one of its backups.
import type { BackupEntry } from '../../application/contracts'

export function restorableBackups(backups: BackupEntry[]): BackupEntry[] {
  return backups.filter(backup => backup.available && backup.revision != null)
}

/** A safety backup of the current state, then the restore. The restore is
 *  committed into the restored copy, so it expects the backup's revision. */
export function backupRestoreRequests(backup: BackupEntry) {
  return [
    { type: 'backup_create' as const, payload: { kind: 'pre_restore' } },
    {
      type: 'backup_restore' as const,
      expectedRevision: backup.revision as number,
      payload: { backupName: backup.backupName, checksumSha256: backup.checksumSha256 },
    },
  ] as const
}
