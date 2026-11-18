import { toast } from 'sonner'
import { errorMessage } from './api'
import { queryClient } from './queryClient'

const UNDO_MS = 6000
const pending = new Map<string, () => Promise<void>>()

/** Runs every pending delete now. Called before a workspace switch so ids never hit the wrong workspace. */
export async function flushUndoableDeletes() {
  await Promise.all([...pending.values()].map((commit) => commit()))
}

if (typeof window !== 'undefined') {
  // shortcut: best effort, the app may close before the backend call completes; upgrade needs a backend soft-delete.
  window.addEventListener('beforeunload', () => void flushUndoableDeletes())
}

/**
 * Removes the item from the cached list at once, offers Undo for 6s, then runs the real delete.
 * shortcut: an unrelated invalidate of listKey while pending re-shows the item until the delete lands.
 */
export function undoableDelete<T extends { id: string }>({
  id,
  name,
  listKey,
  remove,
}: {
  id: string
  name: string
  listKey: readonly unknown[]
  remove: () => Promise<unknown>
}) {
  if (pending.has(id)) return
  const refetch = () => void queryClient.invalidateQueries({ queryKey: listKey })
  void queryClient.cancelQueries({ queryKey: listKey })
  queryClient.setQueryData<T[]>(listKey, (items) => items?.filter((item) => item.id !== id))

  const timer = setTimeout(() => void commit(), UNDO_MS)
  const toastId = toast(`Deleted ${name}`, {
    duration: UNDO_MS + 500,
    action: {
      label: 'Undo',
      onClick: () => {
        clearTimeout(timer)
        pending.delete(id)
        refetch()
      },
    },
  })
  async function commit() {
    if (!pending.delete(id)) return
    clearTimeout(timer)
    toast.dismiss(toastId)
    try {
      await remove()
    } catch (failure) {
      toast.error(errorMessage(failure))
    }
    refetch()
  }
  pending.set(id, commit)
}
