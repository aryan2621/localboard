import { Fragment, useEffect, useMemo, useRef, useState } from 'react'
import { keepPreviousData, useMutation, useQuery } from '@tanstack/react-query'
import { save } from '@tauri-apps/plugin-dialog'
import { ArrowRight, Check, Database, MessageSquare, MessageSquarePlus, Table2, X } from 'lucide-react'
import { toast } from 'sonner'
import { ConfirmDialog, EmptyState, PageContainer, PageHeader, PageLoader, RowMenu, SearchInput } from '../../components/layout/ui'
import { StatRow } from '../../components/layout/patterns'
import { Badge } from '../../components/ui/badge'
import { Button } from '../../components/ui/button'
import { Checkbox } from '../../components/ui/checkbox'
import { DropdownMenuItem, DropdownMenuSeparator } from '../../components/ui/dropdown-menu'
import { Input } from '../../components/ui/input'
import { PaginationBar } from '../../components/ui/pagination'
import { Sheet, SheetBody, SheetContent, SheetFooter, SheetHeader } from '../../components/ui/sheet'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../../components/ui/table'
import { usePagination } from '../../lib/usePagination'
import { api, errorMessage, type ConversationSummary } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { relativeTime } from '../../lib/time'
import { useNavigation } from '../../stores/navigation'

const SEARCH_DEBOUNCE_MS = 200
const DAY_MS = 86_400_000

function groupKey(updatedAt: string): string {
  const start = new Date()
  start.setHours(0, 0, 0, 0)
  const age = start.getTime() - new Date(updatedAt).getTime()
  if (age <= 0) return 'Today'
  if (age <= DAY_MS) return 'Yesterday'
  if (age <= 7 * DAY_MS) return 'Previous 7 days'
  if (age <= 30 * DAY_MS) return 'Previous 30 days'
  return 'Older'
}

function fileName(title: string) {
  return `${title.replace(/[\\/:*?"<>|]+/g, ' ').trim() || 'LocalBoard chat'}.md`
}

function invalidate(id?: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.conversations })
  if (id) void queryClient.invalidateQueries({ queryKey: queryKeys.conversation(id) })
}

async function exportOne(conversation: ConversationSummary) {
  const path = await save({ defaultPath: fileName(conversation.title), filters: [{ name: 'Markdown', extensions: ['md'] }] })
  if (!path) return false
  await api.exportConversation(conversation.id, path)
  return true
}

function Row({
  conversation,
  selected,
  onSelect,
  onPreview,
}: {
  conversation: ConversationSummary
  selected: boolean
  onSelect: (id: string) => void
  onPreview: (id: string) => void
}) {
  const [editing, setEditing] = useState(false)
  const [removeOpen, setRemoveOpen] = useState(false)
  const [title, setTitle] = useState(conversation.title)
  const renameButton = useRef<HTMLButtonElement>(null)

  const rename = useMutation({
    mutationFn: () => api.renameConversation(conversation.id, title.trim()),
    onSuccess: () => {
      setEditing(false)
      invalidate(conversation.id)
      window.setTimeout(() => renameButton.current?.focus(), 0)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const exportChat = useMutation({
    mutationFn: () => exportOne(conversation),
    onSuccess: (saved) => saved && toast.success('Chat exported'),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const remove = useMutation({
    mutationFn: () => api.deleteConversation(conversation.id),
    onSuccess: () => {
      queryClient.removeQueries({ queryKey: queryKeys.conversation(conversation.id) })
      invalidate()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  const cancelEdit = () => {
    setEditing(false)
    setTitle(conversation.title)
    window.setTimeout(() => renameButton.current?.focus(), 0)
  }

  return (
    <TableRow className="group" data-state={selected ? 'selected' : undefined}>
      <TableCell className="w-10">
        <Checkbox checked={selected} onCheckedChange={() => onSelect(conversation.id)} aria-label={`Select ${conversation.title}`} />
      </TableCell>
      <TableCell className="max-w-0">
        {editing ? (
          <form
            className="flex min-w-0 items-center gap-1"
            onSubmit={(event) => {
              event.preventDefault()
              if (title.trim()) rename.mutate()
            }}
          >
            <Input
              autoFocus
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              onKeyDown={(event) => event.key === 'Escape' && cancelEdit()}
              aria-label="Chat name"
            />
            <Button type="submit" variant="ghost" size="icon" aria-label="Save" disabled={!title.trim() || rename.isPending}>
              <Check strokeWidth={iconStroke} />
            </Button>
            <Button type="button" variant="ghost" size="icon" aria-label="Cancel" onClick={cancelEdit}>
              <X strokeWidth={iconStroke} />
            </Button>
          </form>
        ) : (
          <button
            ref={renameButton}
            type="button"
            onClick={() => onPreview(conversation.id)}
            title={conversation.title}
            className="block w-full truncate rounded-md text-left text-app-label text-foreground hover:text-brand focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
          >
            {conversation.title}
          </button>
        )}
      </TableCell>
      <TableCell className="w-40 whitespace-nowrap text-app-caption text-muted-foreground">{relativeTime(conversation.updatedAt)}</TableCell>
      <TableCell className="w-12 text-right">
        <RowMenu label={`Actions for ${conversation.title}`}>
          <DropdownMenuItem onSelect={() => onPreview(conversation.id)}>Preview</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => setEditing(true)}>Rename</DropdownMenuItem>
          <DropdownMenuItem disabled={exportChat.isPending} onSelect={() => exportChat.mutate()}>
            Export as Markdown
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setRemoveOpen(true)}>
            Delete
          </DropdownMenuItem>
        </RowMenu>
        <ConfirmDialog
          open={removeOpen}
          onOpenChange={setRemoveOpen}
          title="Delete this chat?"
          description="Its questions, answers and saved results are removed from this workspace. Your data is not affected."
          confirmLabel="Delete"
          onConfirm={() => remove.mutate()}
        />
      </TableCell>
    </TableRow>
  )
}

/** The drawer body for one chat: numbers, the conversation, and a way back into it. */
function Preview({ id }: { id: string }) {
  const openConversation = useNavigation((state) => state.openConversation)
  const { data, isLoading, error } = useQuery({ queryKey: queryKeys.conversation(id), queryFn: () => api.conversation(id) })
  if (isLoading) {
    return (
      <>
        <SheetHeader title="Loading chat…" />
        <PageLoader />
      </>
    )
  }
  if (error || !data) {
    return (
      <>
        <SheetHeader title="Chat" />
        <EmptyState icon={MessageSquare} title="Couldn't open this chat" description={errorMessage(error)} />
      </>
    )
  }
  const questions = data.messages.filter((message) => message.role === 'user')
  const failed = data.queries.filter((query) => query.status === 'error').length
  const tables = new Set(data.queries.flatMap((query) => query.relations))
  const last = data.messages[data.messages.length - 1]
  return (
    <>
      <SheetHeader title={data.title} description={last ? `Last message ${relativeTime(last.createdAt)}` : undefined} />
      <SheetBody>
        <StatRow
          stats={[
            { label: 'Questions', value: questions.length, icon: MessageSquare },
            { label: 'Queries run', value: data.queries.length, icon: Database, hint: failed ? `${failed} failed` : undefined },
            { label: 'Tables used', value: tables.size, icon: Table2, hint: [...tables].slice(0, 2).join(', ') || undefined },
          ]}
        />
        <ol className="space-y-4">
          {data.messages.map((message) =>
            message.role === 'user' ? (
              <li key={message.id} className="flex justify-end">
                <p className="max-w-[85%] whitespace-pre-wrap rounded-2xl bg-accent px-4 py-2.5 text-app-label text-foreground">{message.content}</p>
              </li>
            ) : (
              <li key={message.id} className="space-y-1">
                <p className="line-clamp-6 whitespace-pre-wrap text-app-label text-foreground">{message.content || (message.status === 'error' ? 'This answer failed.' : '…')}</p>
                {message.feedback && <Badge variant="secondary">{message.feedback === 1 ? 'Marked helpful' : 'Marked not helpful'}</Badge>}
              </li>
            ),
          )}
        </ol>
      </SheetBody>
      <SheetFooter>
        <Button onClick={() => openConversation(id)}>
          Continue chat <ArrowRight strokeWidth={iconStroke} />
        </Button>
      </SheetFooter>
    </>
  )
}

export function ChatHistoryPage() {
  const [draft, setDraft] = useState('')
  const [search, setSearch] = useState('')
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const [bulkOpen, setBulkOpen] = useState(false)
  const [previewId, setPreviewId] = useState<string | null>(null)
  const navigate = useNavigation((state) => state.navigate)
  const { data: conversations, isLoading } = useQuery({
    queryKey: queryKeys.conversationSearch(search),
    queryFn: () => api.conversations(search),
    placeholderData: keepPreviousData,
  })

  useEffect(() => {
    const timer = window.setTimeout(() => setSearch(draft.trim()), SEARCH_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [draft])

  useEffect(() => {
    setPicked(new Set())
  }, [search])

  const all = conversations ?? []
  const pg = usePagination(all, { initialPageSize: 25, resetKey: search })

  // Day groups are built from the current page only.
  const groups = useMemo(() => {
    const out: { key: string; items: ConversationSummary[] }[] = []
    for (const conversation of pg.pageItems) {
      const key = groupKey(conversation.updatedAt)
      const last = out[out.length - 1]
      if (last?.key === key) last.items.push(conversation)
      else out.push({ key, items: [conversation] })
    }
    return out
  }, [pg.pageItems])

  const toggle = (id: string) =>
    setPicked((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  const pageIds = pg.pageItems.map((item) => item.id)
  const allPicked = pageIds.length > 0 && pageIds.every((id) => picked.has(id))
  const togglePage = () =>
    setPicked((current) => {
      const next = new Set(current)
      for (const id of pageIds) {
        if (allPicked) next.delete(id)
        else next.add(id)
      }
      return next
    })

  const bulkDelete = useMutation({
    mutationFn: async () => {
      for (const id of picked) await api.deleteConversation(id)
    },
    onSuccess: () => {
      toast.success(`${picked.size} ${picked.size === 1 ? 'chat' : 'chats'} deleted`)
      setPicked(new Set())
      invalidate()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const bulkExport = useMutation({
    mutationFn: async () => {
      let saved = 0
      for (const id of picked) {
        const conversation = all.find((item) => item.id === id)
        if (conversation && (await exportOne(conversation))) saved += 1
      }
      return saved
    },
    onSuccess: (saved) => saved > 0 && toast.success(`${saved} ${saved === 1 ? 'chat' : 'chats'} exported`),
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  if (isLoading) return <PageLoader />
  if (!search && all.length === 0) {
    return (
      <PageContainer>
        <EmptyState
          icon={MessageSquare}
          title="No chats yet"
          description="Ask a question and it will show up here."
          action={
            <Button onClick={() => navigate('home')}>
              <MessageSquarePlus strokeWidth={iconStroke} /> Start a chat
            </Button>
          }
        />
      </PageContainer>
    )
  }

  return (
    <PageContainer width="full">
      <PageHeader
        title="Chat history"
        description="Every question you asked, newest first. Open one to read it or carry on."
        actions={
          <Button onClick={() => navigate('home')}>
            <MessageSquarePlus strokeWidth={iconStroke} /> New chat
          </Button>
        }
      />
      <div className="flex flex-wrap items-center gap-3">
        <div className="min-w-64 flex-1">
          <SearchInput value={draft} onChange={setDraft} placeholder="Search questions and answers" />
        </div>
        {picked.size > 0 && (
          <div className="flex items-center gap-2">
            <span className="text-app-caption text-foreground">{picked.size} selected</span>
            <Button variant="ghost" size="sm" onClick={() => setPicked(new Set())}>
              Clear
            </Button>
            <Button variant="outline" size="sm" loading={bulkExport.isPending} onClick={() => bulkExport.mutate()}>
              Export
            </Button>
            <Button variant="destructive" size="sm" loading={bulkDelete.isPending} onClick={() => setBulkOpen(true)}>
              Delete
            </Button>
          </div>
        )}
      </div>
      {all.length === 0 ? (
        <p className="py-10 text-center text-app-label text-muted-foreground">No chats match. Try a different search.</p>
      ) : (
        <div className="space-y-3">
          <div className="overflow-hidden rounded-xl border border-border bg-surface">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-10">
                    <Checkbox checked={allPicked} onCheckedChange={togglePage} aria-label="Select all on this page" />
                  </TableHead>
                  <TableHead>Chat</TableHead>
                  <TableHead className="w-40">Updated</TableHead>
                  <TableHead className="w-12" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {groups.map((group) => (
                  <Fragment key={group.key}>
                    <TableRow className="hover:bg-transparent">
                      <TableCell colSpan={4} className="bg-muted/40 py-1.5 text-app-caption font-medium text-muted-foreground">
                        {group.key}
                      </TableCell>
                    </TableRow>
                    {group.items.map((conversation) => (
                      <Row key={conversation.id} conversation={conversation} selected={picked.has(conversation.id)} onSelect={toggle} onPreview={setPreviewId} />
                    ))}
                  </Fragment>
                ))}
              </TableBody>
            </Table>
          </div>
          <PaginationBar alwaysShow page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} />
        </div>
      )}
      <Sheet open={previewId !== null} onOpenChange={(open) => !open && setPreviewId(null)}>
        <SheetContent width="lg">{previewId && <Preview key={previewId} id={previewId} />}</SheetContent>
      </Sheet>
      <ConfirmDialog
        open={bulkOpen}
        onOpenChange={setBulkOpen}
        title={`Delete ${picked.size} ${picked.size === 1 ? 'chat' : 'chats'}?`}
        description="Their questions, answers and saved results are removed from this workspace. Your data is not affected."
        confirmLabel="Delete"
        onConfirm={() => bulkDelete.mutate()}
      />
    </PageContainer>
  )
}
