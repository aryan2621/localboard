import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Download, Trash2 } from 'lucide-react'
import { AnswerInspector, AnswerView, type InspectorTab } from '../../components/analyst/AnswerView'
import { GenerationSteps } from '../../components/analyst/StepsPanel'
import { Composer, type OutputKind } from '../../components/analyst/Composer'
import { Button } from '../../components/ui/button'
import { ConfirmAction } from '../../components/ui/alert-dialog'
import { Alert, AlertDescription } from '../../components/ui/alert'
import { Hint } from '../../components/ui/tooltip'
import { PageLoader } from '../../components/layout/ui'
import { useGenerate } from '../../lib/analyst'
import { useDeleteChat, useExportChat } from '../../lib/conversations'
import { api, errorMessage, type ConversationDetail } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { useAnalyst } from '../../stores/analyst'
import { useNavigation } from '../../stores/navigation'

function ChatHeader({ conversation }: { conversation: ConversationDetail }) {
  const navigate = useNavigation((state) => state.navigate)
  const exportChat = useExportChat(conversation)
  const remove = useDeleteChat(conversation, () => navigate('home'))

  return (
    <header className="flex items-center gap-2 border-b border-border pb-4">
      <Hint text={conversation.title}>
        <h1 className="min-w-0 flex-1 truncate font-medium text-app-heading text-foreground" title={conversation.title}>{conversation.title}</h1>
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

type Inspected = { messageId: string; tab: InspectorTab }

function Conversation({ id, inspected, onInspect }: { id: string; inspected: Inspected | null; onInspect: (next: Inspected | null) => void }) {
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
        {conversation.messages.map((message, index) =>
          message.role === 'user' ? (
            <Question key={message.id}>{message.content}</Question>
          ) : (
            <AnswerView
              key={message.id}
              message={message}
              queries={conversation.queries.filter((query) => query.messageId === message.id)}
              live={live[message.id]}
              onAsk={(question) => void submit(question).catch(() => undefined)}
              canAsk={!writing && !generate.isPending && !sending}
              question={conversation.messages[index - 1]?.role === 'user' ? conversation.messages[index - 1].content : null}
              inspecting={inspected?.messageId === message.id ? inspected.tab : null}
              onInspect={(tab) => onInspect(tab ? { messageId: message.id, tab } : null)}
            />
          ),
        )}
        {sending && (
          <>
            <Question>{sending.question}</Question>
            {sending.output !== 'answer' && <GenerationSteps running />}
          </>
        )}
        <div ref={end} />
      </div>
      <div className="sticky bottom-0 border-t border-border bg-background pb-4 pt-3">
        <Composer onSubmit={submit} busy={generate.isPending || Boolean(sending)} disabled={writing} />
      </div>
    </>
  )
}

/** Each turn opens with its question as a heading, so the thread reads as a list of answers. */
function Question({ children }: { children: string }) {
  return <h2 className="whitespace-pre-wrap break-words border-t border-border pt-6 text-app-title font-semibold text-foreground first:border-t-0 first:pt-0">{children}</h2>
}

export function ChatPage() {
  const conversationId = useNavigation((state) => state.conversationId)
  const navigate = useNavigation((state) => state.navigate)
  const [inspected, setInspected] = useState<Inspected | null>(null)
  const { data: conversation } = useQuery({ queryKey: queryKeys.conversation(conversationId ?? ''), queryFn: () => api.conversation(conversationId ?? ''), enabled: Boolean(conversationId) })
  const message = conversation?.messages.find((item) => item.id === inspected?.messageId)

  useEffect(() => {
    if (!conversationId) navigate('home')
    setInspected(null)
  }, [conversationId, navigate])

  return (
    <div className="flex h-full">
      <div className="min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex min-h-full max-w-3xl flex-col px-4 pt-6 sm:px-8">
          {conversationId && <Conversation key={conversationId} id={conversationId} inspected={inspected} onInspect={setInspected} />}
        </div>
      </div>
      {conversation && message && inspected && (
        <AnswerInspector
          message={message}
          queries={conversation.queries.filter((query) => query.messageId === message.id)}
          tab={inspected.tab}
          onTab={(tab) => setInspected({ messageId: message.id, tab })}
          onClose={() => setInspected(null)}
        />
      )}
    </div>
  )
}
