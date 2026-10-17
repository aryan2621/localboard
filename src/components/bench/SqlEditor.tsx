import { useEffect, useRef, type MutableRefObject } from 'react'
import { autocompletion } from '@codemirror/autocomplete'
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { PostgreSQL, sql } from '@codemirror/lang-sql'
import { bracketMatching, defaultHighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { EditorState } from '@codemirror/state'
import { EditorView, keymap, lineNumbers } from '@codemirror/view'

type Schema = Record<string, string[]>

export function SqlEditor({ value, schema, onChange, onRun, insertRef }: { value: string; schema: Schema; onChange: (sql: string) => void; onRun?: () => void; insertRef?: MutableRefObject<((text: string) => void) | null> }) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<EditorView | null>(null)
  const onChangeRef = useRef(onChange)
  const onRunRef = useRef(onRun)
  const valueRef = useRef(value)
  onChangeRef.current = onChange
  onRunRef.current = onRun
  valueRef.current = value

  useEffect(() => {
    if (!host.current) return
    const editor = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: valueRef.current,
        extensions: [
          lineNumbers(),
          history(),
          bracketMatching(),
          syntaxHighlighting(defaultHighlightStyle),
          autocompletion(),
          keymap.of([
            // Cmd/Ctrl+Enter runs, the way every SQL console does.
            { key: 'Mod-Enter', preventDefault: true, run: () => (onRunRef.current?.(), true) },
            ...defaultKeymap,
            ...historyKeymap,
          ]),
          sql({ dialect: PostgreSQL, schema, upperCaseKeywords: true }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onChangeRef.current(update.state.doc.toString())
          }),
          EditorView.theme({
            '&': { fontSize: 'var(--app-text-caption)', backgroundColor: 'transparent' },
            '.cm-scroller': { fontFamily: 'JetBrains Mono Variable, ui-monospace, monospace', minHeight: '180px' },
            '.cm-gutters': { backgroundColor: 'transparent', border: 'none' },
          }),
        ],
      }),
    })
    view.current = editor
    if (insertRef) {
      insertRef.current = (text) => {
        const { from, to } = editor.state.selection.main
        editor.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length } })
        editor.focus()
      }
    }
    return () => {
      if (insertRef) insertRef.current = null
      editor.destroy()
      view.current = null
    }
  }, [schema])

  useEffect(() => {
    const editor = view.current
    if (!editor) return
    const current = editor.state.doc.toString()
    if (current !== value) editor.dispatch({ changes: { from: 0, to: current.length, insert: value } })
  }, [value])

  return <div ref={host} className="overflow-hidden rounded-lg border border-input bg-surface" />
}
