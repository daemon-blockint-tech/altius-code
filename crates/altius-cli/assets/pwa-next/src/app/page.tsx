'use client'

import { useEffect, useState, useCallback, useMemo, useRef } from 'react'
import { TaskSidebar } from '@/components/task-sidebar'
import { ChatPanel } from '@/components/chat-panel'
import { TaskInput } from '@/components/task-input'
import { StatusBar } from '@/components/status-bar'
import { AcpClient, userText, agentText } from '@/lib/acp-client'
import type { Run, Message, AgentCard } from '@/lib/types'
import { Plus, Menu, Sun, Moon, X, AlertTriangle } from 'lucide-react'

interface Toast {
  id: number
  message: string
  type: 'error' | 'info'
}

export default function Home() {
  const [apiBase, setApiBase] = useState('')
  const [token, setToken] = useState('')
  const [runs, setRuns] = useState<Run[]>([])
  const [activeRun, setActiveRun] = useState<Run | null>(null)
  const [messages, setMessages] = useState<Message[]>([])
  const [busy, setBusy] = useState(false)
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const [health, setHealth] = useState<'ok' | 'down' | 'checking'>('checking')
  const [loadingRuns, setLoadingRuns] = useState(true)
  const [agentCard, setAgentCard] = useState<AgentCard | null>(null)
  const [selectedAgent, setSelectedAgent] = useState('fleet-supervisor')
  const [darkMode, setDarkMode] = useState(false)
  const [toasts, setToasts] = useState<Toast[]>([])
  const [isMobile, setIsMobile] = useState(false)
  const toastIdRef = useRef(0)

  const client = useMemo(() => new AcpClient(apiBase, token), [apiBase, token])

  const showToast = useCallback((message: string, type: 'error' | 'info' = 'error') => {
    const id = ++toastIdRef.current
    setToasts((prev) => [...prev, { id, message, type }])
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 5000)
  }, [])

  // Resolve API base from window.location + load env overrides
  useEffect(() => {
    const envBase = process.env.NEXT_PUBLIC_API_URL || ''
    const base = envBase || (typeof window !== 'undefined' ? window.location.origin : '')
    setApiBase(base)
    const t = process.env.NEXT_PUBLIC_AUTH_TOKEN || ''
    setToken(t)

    // Dark mode from localStorage
    const saved = localStorage.getItem('altius-dark')
    if (saved === 'true') {
      setDarkMode(true)
      document.documentElement.classList.add('dark')
    }

    // Mobile detection
    const checkMobile = () => setIsMobile(window.innerWidth < 768)
    checkMobile()
    window.addEventListener('resize', checkMobile)
    return () => window.removeEventListener('resize', checkMobile)
  }, [])

  const refreshRuns = useCallback(async () => {
    try {
      const list = await client.listRuns()
      setRuns(list)
      setHealth('ok')
    } catch {
      setHealth('down')
    } finally {
      setLoadingRuns(false)
    }
  }, [client])

  // Fetch agent card
  useEffect(() => {
    if (!apiBase) return
    client.getAgentCard().then(setAgentCard).catch(() => {})
  }, [apiBase, client])

  // Poll runs
  useEffect(() => {
    if (!apiBase) return
    refreshRuns()
    const interval = setInterval(refreshRuns, 3000)
    return () => clearInterval(interval)
  }, [apiBase, refreshRuns])

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault()
        newChat()
      }
      if (e.key === 'Escape') {
        if (isMobile) setSidebarOpen(false)
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [isMobile])

  const toggleDarkMode = () => {
    const next = !darkMode
    setDarkMode(next)
    document.documentElement.classList.toggle('dark', next)
    localStorage.setItem('altius-dark', String(next))
  }

  // Poll a run until terminal (fallback when SSE ends early)
  const pollUntilTerminal = useCallback(async (runId: string, maxAttempts = 150) => {
    for (let i = 0; i < maxAttempts; i++) {
      await new Promise((r) => setTimeout(r, 2000))
      try {
        const r = await client.getRun(runId)
        setActiveRun(r)
        if (r.status === 'completed') {
          setMessages((prev) => [...prev, ...r.output])
          return
        }
        if (r.status === 'failed') {
          setMessages((prev) => [...prev, agentText(`Error: ${r.error || 'unknown'}`)])
          return
        }
        if (r.status === 'cancelled') {
          setMessages((prev) => [...prev, agentText('Run cancelled.')])
          return
        }
        if (r.status === 'awaiting' && r.approval) {
          const ap = r.approval
          setMessages((prev) => [...prev, agentText(ap.summary)])
          return
        }
      } catch {
        // keep polling
      }
    }
  }, [client])

  const handleSubmit = async (prompt: string) => {
    if (!prompt.trim() || busy) return
    setBusy(true)
    const userMsg = userText(prompt)
    setMessages((prev) => [...prev, userMsg])

    try {
      const run = await client.createRun(selectedAgent, [userMsg])
      setActiveRun(run)
      setRuns((prev) => [run, ...prev])

      let streamEnded = false
      let lastStatus = run.status

      try {
        for await (const event of client.streamEvents(run.run_id)) {
          const r = event.run
          setActiveRun(r)
          lastStatus = r.status
          if (r.status === 'awaiting' && r.approval) {
            const ap = r.approval
            setMessages((prev) => [...prev, agentText(ap.summary)])
          }
          if (r.status === 'completed') {
            setMessages((prev) => [...prev, ...r.output])
          }
          if (r.status === 'failed') {
            setMessages((prev) => [...prev, agentText(`Error: ${r.error || 'unknown'}`)])
          }
        }
        streamEnded = true
      } catch {
        // SSE failed — will fall back to polling
      }

      // If SSE ended but run is not terminal, poll as fallback
      if (streamEnded && lastStatus && !['completed', 'failed', 'cancelled'].includes(lastStatus)) {
        await pollUntilTerminal(run.run_id)
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      showToast(`Failed: ${msg}`)
      setMessages((prev) => [...prev, agentText(`Failed: ${msg}`)])
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
      for await (const event of client.streamEvents(run.run_id)) {
        const r = event.run
        setActiveRun(r)
        if (r.status === 'completed') {
          setMessages((prev) => [...prev, ...r.output])
        }
        if (r.status === 'failed') {
          setMessages((prev) => [...prev, agentText(`Error: ${r.error || 'unknown'}`)])
        }
      }
    } catch (err) {
      showToast(`Resume failed: ${err}`)
    } finally {
      setBusy(false)
      refreshRuns()
    }
  }

  const handleCancel = async () => {
    if (!activeRun) return
    try {
      await client.cancelRun(activeRun.run_id)
      setMessages((prev) => [...prev, agentText('Run cancelled.')])
    } catch (err) {
      showToast(`Cancel failed: ${err}`)
    } finally {
      setBusy(false)
      refreshRuns()
    }
  }

  const selectRun = (run: Run) => {
    setActiveRun(run)
    setMessages([...run.input, ...run.output])
    if (isMobile) setSidebarOpen(false)
  }

  const newChat = () => {
    setActiveRun(null)
    setMessages([])
  }

  const agentSkills = agentCard?.skills || []

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-bg-primary text-text-primary">
      {/* Header */}
      <header className="flex items-center gap-3 border-b border-border-subtle px-4 py-2.5 bg-bg-elevated z-20">
        <button
          onClick={() => setSidebarOpen(!sidebarOpen)}
          className="p-1.5 rounded-md hover:bg-bg-secondary transition-colors"
        >
          <Menu className="w-4 h-4" />
        </button>
        <h1 className="font-display text-lg font-semibold tracking-tight">Altius Fleet</h1>
        <div className="ml-auto flex items-center gap-2">
          <button
            onClick={toggleDarkMode}
            className="p-1.5 rounded-md hover:bg-bg-secondary transition-colors"
            title="Toggle dark mode"
          >
            {darkMode ? <Sun className="w-4 h-4" /> : <Moon className="w-4 h-4" />}
          </button>
          <button
            onClick={newChat}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-md bg-accent text-white text-sm font-medium hover:opacity-90 transition-opacity"
            title="New chat (Cmd+K)"
          >
            <Plus className="w-4 h-4" />
            New
          </button>
        </div>
      </header>

      {/* Disconnected banner */}
      {health === 'down' && (
        <div className="flex items-center gap-2 px-4 py-2 bg-bad-soft border-b border-bad/30 text-sm text-bad">
          <AlertTriangle className="w-4 h-4 flex-shrink-0" />
          <span>Server disconnected</span>
          <button
            onClick={() => refreshRuns()}
            className="ml-auto px-2 py-0.5 rounded text-xs font-medium bg-bad text-white hover:opacity-90"
          >
            Retry
          </button>
        </div>
      )}

      {/* Main layout */}
      <div className="flex flex-1 overflow-hidden relative">
        {/* Mobile backdrop */}
        {isMobile && sidebarOpen && (
          <div
            className="fixed inset-0 bg-black/30 z-10"
            onClick={() => setSidebarOpen(false)}
          />
        )}

        {sidebarOpen && (
          <TaskSidebar
            runs={runs}
            activeRunId={activeRun?.run_id ?? null}
            onSelect={selectRun}
            loading={loadingRuns}
            className={isMobile ? 'fixed left-0 top-0 bottom-0 z-20 shadow-lg' : ''}
          />
        )}

        {/* Chat area */}
        <main className="flex flex-1 flex-col overflow-hidden">
          <ChatPanel
            messages={messages}
            busy={busy}
            awaitingApproval={activeRun?.status === 'awaiting'}
            approval={activeRun?.approval ?? null}
            onApprove={handleApprove}
            onCancel={handleCancel}
            canCancel={busy && activeRun && !['completed', 'failed', 'cancelled'].includes(activeRun.status)}
          />
          <TaskInput
            onSubmit={handleSubmit}
            disabled={busy}
            agents={agentSkills}
            selectedAgent={selectedAgent}
            onAgentChange={setSelectedAgent}
          />
        </main>
      </div>

      <StatusBar health={health} apiBase={apiBase} agentCount={agentSkills.length} />

      {/* Toasts */}
      <div className="fixed bottom-12 right-4 z-50 space-y-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`flex items-center gap-2 px-4 py-2.5 rounded-md shadow-md text-sm ${
              t.type === 'error'
                ? 'bg-bad text-white'
                : 'bg-bg-elevated border border-border-subtle text-text-primary'
            }`}
          >
            <span>{t.message}</span>
            <button
              onClick={() => setToasts((prev) => prev.filter((x) => x.id !== t.id))}
              className="ml-2 opacity-70 hover:opacity-100"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        ))}
      </div>
    </div>
  )
}
