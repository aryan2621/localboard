import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { save } from '@tauri-apps/plugin-dialog'
import { Download, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { AnswerView } from '../../components/analyst/AnswerView'
import { GenerationSteps } from '../../components/analyst/StepsPanel'
import { Composer, type OutputKind } from '../../components/analyst/Composer'
import { Button } from '../../components/ui/button'
import { ConfirmAction } from '../../components/ui/alert-dialog'
import { Alert, AlertDescription } from '../../components/ui/alert'
import { Hint } from '../../components/ui/tooltip'
import { PageLoader } from '../../components/layout/ui'
import { useGenerate } from '../../lib/analyst'
import { api, errorMessage, type ConversationDetail } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { useAnalyst } from '../../stores/analyst'
import { useNavigation } from '../../stores/navigation'

function fileName(title: string) {
  return `${title.replace(/[\\/:*?"<>|]+/g, ' ').trim() || 'LocalBoard chat'}.md`
}

function ChatHeader({ conversation }: { conversation: ConversationDetail }) {
  const navigate = useNavigation((state) => state.navigate)
  const exportChat = useMutation({
    mutationFn: async () => {
      const path = await save({ defaultPath: fileName(conversation.title), filters: [{ name: 'Markdown', extensions: ['md'] }] })
      if (!path) return false
      await api.exportConversation(conversation.id, path)
      return true
    },
    onSuccess: (saved) => saved && toast.success('Chat exported'),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteConversation(conversation.id),
    onSuccess: () => {
      navigate('home')
      queryClient.removeQueries({ queryKey: queryKeys.conversation(conversation.id) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.conversations })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  return (
    <header className="flex items-center gap-2 border-b border-border pb-4">
      <Hint text={conversation.title}>
        <h1 className="min-w-0 flex-1 truncate font-serif text-app-heading text-foreground">{conversation.title}</h1>
      </Hint>
      <Button variant="ghost" size="sm" onClick={() => exportChat.mutate()} disabled={exportChat.isPending}>
        <Download strokeWidth={iconStroke} /> {'Export'}
      </Button>
      <ConfirmAction
        trigger={
          <Button variant="ghost" size="icon" aria-label={'Delete chat'} disabled={remove.isPending}>
            <Trash2 strokeWidth={iconStroke} />
          </Button>
        }
        hint="Delete chat"
        title={'Delete this chat?'}
        description={'Its questions, answers and saved results are removed from this workspace. Your data is not affected.'}
        confirmLabel={'Delete'}
        onConfirm={() => remove.mutate()}
      />
    </header>
  )
}

function Conversation({ id }: { id: string }) {
  const { data: conversation, error } = useQuery({ queryKey: queryKeys.conversation(id), queryFn: () => api.conversation(id) })
  const live = useAnalyst((state) => state.live)
  const generate = useGenerate()
  const end = useRef<HTMLDivElement>(null)
  const messageCount = conversation?.messages.length ?? 0
  const writing = conversation?.messages.some((message) => message.status === 'pending') ?? false
  // The message just sent, shown at once until the stored copy (or the finished artifact) takes over.
  const [sending, setSending] = useState<{ question: string; output: OutputKind } | null>(null)
  const submit = async (question: string, output: OutputKind = 'answer') => {
    setSending({ question, output })
    try {
      return await generate.mutateAsync({ question, conversationId: id, output })
    } finally {
      if (output === 'answer') await queryClient.invalidateQueries({ queryKey: queryKeys.conversation(id) })
      setSending(null)
    }
  }

  useEffect(() => {
    end.current?.scrollIntoView({ block: 'end' })
  }, [messageCount, sending])

  if (error)
    return (
      <Alert variant="destructive" className="my-10">
        <AlertDescription>{errorMessage(error)}</AlertDescription>
      </Alert>
    )
  if (!conversation) return <PageLoader label={'Loading chat'} />

  return (
    <>
      <ChatHeader conversation={conversation} />
      <div className="flex-1 space-y-8 py-6">
        {conversation.messages.map((message) =>
          message.role === 'user' ? (
            <div key={message.id} className="flex justify-end">
              <p className="max-w-[85%] whitespace-pre-wrap rounded-2xl bg-card px-4 py-2.5 text-app-body text-foreground">{message.content}</p>
            </div>
          ) : (
            <AnswerView
              key={message.id}
              message={message}
              queries={conversation.queries.filter((query) => query.messageId === message.id)}
              live={live[message.id]}
              onAsk={(question) => void submit(question).catch(() => undefined)}
              canAsk={!writing && !generate.isPending && !sending}
            />
          ),
        )}
        {sending && (
          <>
            <div className="flex justify-end">
              <p className="max-w-[85%] whitespace-pre-wrap rounded-2xl bg-card px-4 py-2.5 text-app-body text-foreground">{sending.question}</p>
            </div>
            {sending.output !== 'answer' && <GenerationSteps running />}
          </>
        )}
        <div ref={end} />
      </div>
      <div className="sticky bottom-0 bg-gradient-to-t from-background via-background to-transparent pb-6 pt-4">
        <Composer onSubmit={submit} busy={generate.isPending || Boolean(sending)} disabled={writing} />
      </div>
    </>
  )
}

export function ChatPage() {
  const conversationId = useNavigation((state) => state.conversationId)
  const navigate = useNavigation((state) => state.navigate)

  useEffect(() => {
    if (!conversationId) navigate('home')
  }, [conversationId, navigate])

  return (
    <div className="mx-auto flex min-h-[calc(100%-28px)] max-w-3xl flex-col px-8 pt-6">
      {conversationId && <Conversation key={conversationId} id={conversationId} />}
    </div>
  )
}
