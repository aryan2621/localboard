import { useState } from 'react'
import { api } from '../../lib/api'
import { useWorkspaceAction } from '../../lib/workspaces'
import { FormSheet } from '../ui/sheet'
import { Input } from '../ui/input'

export function NewWorkspaceDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const [name, setName] = useState('')
  const action = useWorkspaceAction()

  const submit = () => {
    action.mutate(() => api.createWorkspace(name.trim()), {
      onSuccess: () => {
        setName('')
        onOpenChange(false)
      },
    })
  }

  return (
    <FormSheet
      open={open}
      onOpenChange={onOpenChange}
      title="New workspace"
      description="Each workspace keeps its own connections, tables and history, stored separately on this computer."
      submitLabel="Create"
      onSubmit={() => submit()}
      submitting={action.isPending}
      canSubmit={Boolean(name.trim())}
    >
      <Input autoFocus value={name} maxLength={80} placeholder="e.g. Finance" aria-label="Workspace name" onChange={(e) => setName(e.target.value)} />
    </FormSheet>
  )
}
