'use client'

import { useEffect, useState, useCallback } from 'react'
import { TaskSidebar } from '@/components/task-sidebar'
import { ChatPanel } from '@/components/chat-panel'
import { TaskInput } from '@/components/task-input'
import { StatusBar } from '@/components/status-bar'
import { AcpClient, userText, agentText } from '@/lib/acp-client'
import type { Run, Message } from '@/lib/types'
import { Plus, Menu } from 'lucide-react'

export default function Home() {
  const [apiBase, setApiBase] = useState('')
  const [token, setToken] = useState('')
  const [runs, setRuns] = useState<Run[]>([])
  const [activeRun, setActiveRun] = useState<Run | null>(null)
  const [messages, setMessages] = useState<Message[]>([])
  const [busy, setBusy] = useState(false)
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const [health, setHealth] = useState<'ok' | 'down' | 'checking'>('checking')

  const client = new AcpClient(apiBase, token)

  const refreshRuns = useCallback(async () => {
    try {
      const list = await client.listRuns()
      setRuns(list)
      setHealth('ok')
    } catch {
      setHealth('down')
    }
  }, [apiBase, token])

  useEffect(() => {
    const base = process.env.NEXT_PUBLIC_API_URL || ''
    setApiBase(base)
    const t = process.env.NEXT_PUBLIC_AUTH_TOKEN || ''
    setToken(t)
  }, [])

  useEffect(() => {
    if (!apiBase) return
    refreshRuns()
    const interval = setInterval(refreshRuns, 3000)
    return () => clearInterval(interval)
  }, [apiBase, refreshRuns])

  const handleSubmit = async (prompt: string) => {
    if (!prompt.trim() || busy) return
    setBusy(true)
    const userMsg = userText(prompt)
    setMessages((prev) => [...prev, userMsg])

    try {
      const run = await client.createRun('supervisor', [userMsg])
      setActiveRun(run)
      setRuns((prev) => [run, ...prev])

      // Stream SSE events
      for await (const event of client.streamEvents(run.run_id)) {
        if (event.type === 'run') {
          setActiveRun(event.run)
          if (event.run.status === 'awaiting' && event.run.approval) {
            setMessages((prev) => [
              ...prev,
              agentText(event.run.approval!.summary),
            ])
          }
          if (event.run.status === 'completed') {
            setMessages((prev) => [...prev, ...event.run.output])
          }
          if (event.run.status === 'failed') {
            setMessages((prev) => [
              ...prev,
              agentText(`Error: ${event.run.error || 'unknown'}`),
            ])
          }
        } else if (event.type === 'message') {
          setMessages((prev) => [...prev, event.message])
        }
      }
    } catch (err) {
      setMessages((prev) => [
        ...prev,
        agentText(`Failed: ${err instanceof Error ? err.message : String(err)}`),
      ])
    } finally {
      setBusy(false)
      refreshRuns()
    }
  }

  const handleApprove = async (approved: boolean) => {
    if (!activeRun) return
    setBusy(true)
    try {
      const run = await client.resumeRun(activeRun.run_id, {
        decision: { approved },
      })
      setActiveRun(run)
      setMessages((prev) => [
        ...prev,
        agentText(approved ? 'Approved — continuing...' : 'Denied.'),
      ])
      // Re-stream events
      for await (const event of client.streamEvents(run.run_id)) {
        if (event.type === 'run') {
          setActiveRun(event.run)
          if (event.run.status === 'completed') {
            setMessages((prev) => [...prev, ...event.run.output])
          }
        }
      }
    } catch (err) {
      setMessages((prev) => [
        ...prev,
        agentText(`Resume failed: ${err}`),
      ])
    } finally {
      setBusy(false)
      refreshRuns()
    }
  }

  const selectRun = (run: Run) => {
    setActiveRun(run)
    setMessages([...run.input, ...run.output])
  }

  const newChat = () => {
    setActiveRun(null)
    setMessages([])
  }

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-bg-primary text-text-primary">
      {/* Header */}
      <header className="flex items-center gap-3 border-b border-border-subtle px-4 py-2.5 bg-bg-elevated">
        <button
          onClick={() => setSidebarOpen(!sidebarOpen)}
          className="p-1.5 rounded-md hover:bg-bg-secondary transition-colors"
        >
          <Menu className="w-4 h-4" />
        </button>
        <h1 className="font-display text-lg font-semibold tracking-tight">Altius Fleet</h1>
        <div className="ml-auto flex items-center gap-3">
          <button
            onClick={newChat}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-accent text-white text-sm font-medium hover:opacity-90 transition-opacity"
          >
            <Plus className="w-4 h-4" />
            New
          </button>
        </div>
      </header>

      {/* Main layout */}
      <div className="flex flex-1 overflow-hidden">
        {sidebarOpen && (
          <TaskSidebar
            runs={runs}
            activeRunId={activeRun?.run_id ?? null}
            onSelect={selectRun}
          />
        )}

        {/* Chat area */}
        <main className="flex flex-1 flex-col overflow-hidden">
          <ChatPanel
            messages={messages}
            busy={busy}
            awaitingApproval={activeRun?.status === 'awaiting'}
            onApprove={handleApprove}
          />
          <TaskInput onSubmit={handleSubmit} disabled={busy} />
        </main>
      </div>

      <StatusBar health={health} apiBase={apiBase} />
    </div>
  )
}
