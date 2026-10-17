import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'
import { useAnalyst } from '../stores/analyst'
import { useGeneration } from '../stores/generation'
import { useNavigation } from '../stores/navigation'
import type { OutputKind } from '../components/analyst/Composer'
import { api, errorMessage, type AnalystEventPayload } from './api'
import { queryClient, queryKeys } from './queryClient'

/** Streams progress into the live store; on completion, swaps it for the stored answer. */
export function handleAnalystEvent({ conversationId, messageId, event }: AnalystEventPayload) {
  const { apply, clear } = useAnalyst.getState()
  if (event.type !== 'done') {
    apply(messageId, event)
    return
  }
  void queryClient.invalidateQueries({ queryKey: queryKeys.conversations })
  void queryClient.invalidateQueries({ queryKey: queryKeys.completions })
  void queryClient.invalidateQueries({ queryKey: queryKeys.agents })
  void queryClient
    .invalidateQueries({ queryKey: queryKeys.conversation(conversationId) })
    .finally(() => clear(messageId))
}

export function useAsk() {
  const openConversation = useNavigation((state) => state.openConversation)
  return useMutation({
    mutationFn: ({ question, conversationId }: { question: string; conversationId: string | null }) =>
      api.ask(question, conversationId),
    onSuccess: ({ conversationId }) => {
      openConversation(conversationId)
      void queryClient.invalidateQueries({ queryKey: queryKeys.conversation(conversationId) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.conversations })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
}

export function useGenerate() {
  const openDashboard = useNavigation((state) => state.openDashboard)
  const openDatasite = useNavigation((state) => state.openDatasite)
  const openReport = useNavigation((state) => state.openReport)
  const openDeck = useNavigation((state) => state.openDeck)
  const openAutomation = useNavigation((state) => state.openAutomation)
  return useMutation({
    mutationFn: async ({
      question,
      conversationId,
      output,
    }: {
      question: string
      conversationId: string | null
      output: OutputKind
    }) => {
      if (output !== 'answer') useGeneration.getState().reset()
      if (output === 'answer') {
        const started = await api.ask(question, conversationId)
        return { output, id: started.conversationId }
      }
      if (output === 'dashboard') {
        const live = await api.dashboardGenerate(question)
        if (live.warnings.length) toast.message(live.warnings.join(' '))
        return { output, id: live.dashboard.id }
      }
      if (output === 'datasite') {
        const live = await api.datasiteGenerate(question)
        if (live.warnings.length) toast.message(live.warnings.join(' '))
        return { output, id: live.datasite.id }
      }
      if (output === 'report') {
        const live = await api.reportGenerate(question)
        if (live.warnings.length) toast.message(live.warnings.join(' '))
        return { output, id: live.report.id }
      }
      if (output === 'automation') {
        const draft = await api.automationPropose(question)
        return { output, id: draft.id }
      }
      const live = await api.deckGenerate(question)
      if (live.warnings.length) toast.message(live.warnings.join(' '))
      return { output, id: live.deck.id }
    },
    onSuccess: ({ output, id }) => {
      if (output === 'answer') {
        void queryClient.invalidateQueries({ queryKey: queryKeys.conversation(id) })
        void queryClient.invalidateQueries({ queryKey: queryKeys.conversations })
        useNavigation.getState().openConversation(id)
        return
      }
      if (output === 'dashboard') {
        void queryClient.invalidateQueries({ queryKey: queryKeys.dashboards })
        openDashboard(id)
        return
      }
      if (output === 'datasite') {
        void queryClient.invalidateQueries({ queryKey: queryKeys.datasites })
        openDatasite(id)
        return
      }
      if (output === 'report') {
        void queryClient.invalidateQueries({ queryKey: queryKeys.reports })
        openReport(id)
        return
      }
      if (output === 'automation') {
        void queryClient.invalidateQueries({ queryKey: queryKeys.automations })
        toast.message('Review the draft automation before enabling it')
        openAutomation(id)
        return
      }
      void queryClient.invalidateQueries({ queryKey: queryKeys.decks })
      openDeck(id)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
}
