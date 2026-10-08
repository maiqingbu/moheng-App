import { describe, expect, it } from 'vitest'
import { commandMessage } from '../../src/ui/presentation/productLanguage'

describe('B4-B RV-33 formal responsibility source feedback', () => {
  it('directs contract responsibility edits to ChangeRequest', () => {
    expect(commandMessage({
      status: 'failed',
      message: 'B08_RESPONSIBILITY_CHANGE_REQUEST_REQUIRED',
    })).toContain('变更请求')
  })
})
