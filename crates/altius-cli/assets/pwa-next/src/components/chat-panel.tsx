'use client'

import { useEffect, useRef } from 'react'
import type { Message } from '@/lib/types'
import { partsText, cn } from '@/lib/utils'
import { Check, X, Loader2 } from 'lucide-react'

interface ChatPanelProps {
  messages: Message[]
  busy: boolean
  awaitingApproval: boolean
  onApprove: (approved: boolean) => void
}

export function ChatPanel({ messages, busy, awaitingApproval, onApprove }: ChatPanelProps) {
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [messages, busy])

  return (
    <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-4">
      {messages.length === 0 ? (
        <div className="flex h-full items-center justify-center">
          <div className="text-center">
            <h2 className="font-display text-2xl font-semibold text-text-primary mb-2">
              Altius Fleet
            </h2>
            <p className="text-text-secondary text-sm">
              Send a message to start a new agent run.
            </p>
          </div>
        </div>
      ) : (
        <div className="max-w-3xl mx-auto space-y-4">
          {messages.map((msg, i) => {
            const text = partsText(msg.parts)
            const isUser = msg.role === 'user'
            return (
              <div
                key={i}
                className={cn(
                  'flex flex-col',
                  isUser ? 'items-end' : 'items-start',
                )}
              >
                <div
                  className={cn(
                    'rounded-lg px-4 py-2.5 max-w-[85%] text-sm leading-relaxed',
                    isUser
                      ? 'bg-accent text-white rounded-br-sm'
                      : 'bg-bg-elevated border border-border-subtle rounded-bl-sm',
                  )}
                >
                  <pre className="whitespace-pre-wrap font-body m-0">{text}</pre>
                </div>
              </div>
            )
          })}

          {busy && (
            <div className="flex items-center gap-2 text-text-secondary text-sm">
              <Loader2 className="w-4 h-4 animate-spin" />
              <span>Working...</span>
            </div>
          )}

          {awaitingApproval && (
            <div className="flex items-center gap-3 p-3 rounded-lg bg-warn-soft border border-warn/30">
              <span className="text-sm text-warn font-medium">Approval required</span>
              <div className="flex gap-2 ml-auto">
                <button
                  onClick={() => onApprove(true)}
                  className="flex items-center gap-1 px-3 py-1.5 rounded-md bg-ok text-white text-sm font-medium hover:opacity-90 transition-opacity"
                >
                  <Check className="w-4 h-4" />
                  Approve
                </button>
                <button
                  onClick={() => onApprove(false)}
                  className="flex items-center gap-1 px-3 py-1.5 rounded-md bg-bad text-white text-sm font-medium hover:opacity-90 transition-opacity"
                >
                  <X className="w-4 h-4" />
                  Deny
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
