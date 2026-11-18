import { useMutation } from '@tanstack/react-query'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { api, errorMessage } from './api'
import { queryClient, queryKeys } from './queryClient'

type ChatRef = { id: string; title: string }

function fileName(title: string) {
  return `${title.replace(/[\\/:*?"<>|]+/g, ' ').trim() || 'LocalBoard chat'}.md`
}

export function invalidateConversations(id?: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.conversations })
  if (id) void queryClient.invalidateQueries({ queryKey: queryKeys.conversation(id) })
}

/** Asks where to save, then writes the chat as markdown. Resolves false when the dialog is cancelled. */
export async function exportChat(chat: ChatRef) {
  const path = await save({ defaultPath: fileName(chat.title), filters: [{ name: 'Markdown', extensions: ['md'] }] })
  if (!path) return false
  await api.exportConversation(chat.id, path)
  return true
}

export function useExportChat(chat: ChatRef) {
  return useMutation({
    mutationFn: () => exportChat(chat),
    onSuccess: (saved) => saved && toast.success('Chat exported'),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
}

export function useDeleteChat(chat: ChatRef, onDeleted?: () => void) {
  return useMutation({
    mutationFn: () => api.deleteConversation(chat.id),
    onSuccess: () => {
      onDeleted?.()
      queryClient.removeQueries({ queryKey: queryKeys.conversation(chat.id) })
      invalidateConversations()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
}
