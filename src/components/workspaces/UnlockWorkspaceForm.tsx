import { useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'
import { api, errorMessage } from '../../lib/api'
import { invalidateWorkspaceData, queryClient, queryKeys } from '../../lib/queryClient'
import { useNavigation } from '../../stores/navigation'
import { Button } from '../ui/button'
import { Input } from '../ui/input'
import { Checkbox } from '../ui/checkbox'
import { Label } from '../ui/label'

/** Passphrase prompt for an encrypted workspace; optionally remembers the key in the OS keychain. */
export function UnlockWorkspaceForm({ id, onDone }: { id: string; onDone?: () => void }) {
  const [passphrase, setPassphrase] = useState('')
  const [remember, setRemember] = useState(false)
  const resetForWorkspace = useNavigation((state) => state.resetForWorkspace)
  const unlock = useMutation({
    mutationFn: () => api.unlockWorkspace(id, passphrase, remember),
    onSuccess: () => {
      setPassphrase('')
      void queryClient.invalidateQueries({ queryKey: queryKeys.status })
      void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
      resetForWorkspace()
      invalidateWorkspaceData()
      onDone?.()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <form
      className="w-full max-w-sm space-y-4"
      onSubmit={(event) => {
        event.preventDefault()
        unlock.mutate()
      }}
    >
      <Input type="password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} aria-label="Workspace passphrase" placeholder="Workspace passphrase" autoFocus />
      <div className="flex items-center gap-2">
        <Checkbox id="unlock-remember" checked={remember} onCheckedChange={(value) => setRemember(value === true)} />
        <Label htmlFor="unlock-remember" className="font-normal text-muted-foreground">
          Remember on this computer (OS keychain)
        </Label>
      </div>
      <Button type="submit" disabled={!passphrase || unlock.isPending}>
        Unlock workspace
      </Button>
    </form>
  )
}
