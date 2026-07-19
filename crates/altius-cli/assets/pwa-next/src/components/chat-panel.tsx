'use client'

import { useEffect, useRef, useState, useCallback } from 'react'
import type { Message, RunApproval } from '@/lib/types'
import { partsText, cn } from '@/lib/utils'
import { Check, X, Loader2, Copy, Square } from 'lucide-react'

interface ChatPanelProps {
  messages: Message[]
  busy: boolean
  awaitingApproval: boolean
  approval: RunApproval | null
  onApprove: (approved: boolean) => void
  onCancel: () => void
  canCancel: boolean | null
}

export function ChatPanel({ messages, busy, awaitingApproval, approval, onApprove, onCancel, canCancel }: ChatPanelProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const [copiedIdx, setCopiedIdx] = useState<number | null>(null)

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [messages, busy])

  const handleCopy = useCallback((text: string, idx: number) => {
    navigator.clipboard.writeText(text)
    setCopiedIdx(idx)
    setTimeout(() => setCopiedIdx(null), 2000)
  }, [])

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
            const isTool = msg.role === 'tool'
            const isSystem = msg.role === 'system'

            if (isSystem) {
              return (
                <div key={i} className="flex justify-center">
                  <span className="text-xs text-text-tertiary bg-bg-secondary px-3 py-1 rounded-full">
                    {text}
                  </span>
                </div>
              )
            }

            if (isTool) {
              return (
                <div key={i} className="flex flex-col items-start">
                  <div className="rounded-lg px-3 py-2 max-w-[85%] text-xs font-mono bg-bg-secondary border border-border-subtle text-text-secondary rounded-bl-sm">
                    <span className="text-text-tertiary font-sans text-[10px] uppercase tracking-wide mr-2">Tool</span>
                    <pre className="whitespace-pre-wrap m-0 mt-1">{text}</pre>
                  </div>
                </div>
              )
            }

            return (
              <div
                key={i}
                className={cn(
                  'group flex flex-col',
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
                {!isUser && text && (
                  <div className="flex items-center gap-2 mt-1 opacity-0 group-hover:opacity-100 transition-opacity">
                    <button
                      onClick={() => handleCopy(text, i)}
                      className="text-xs text-text-tertiary hover:text-text-secondary flex items-center gap-1"
                    >
                      {copiedIdx === i ? (
                        <><Check className="w-3 h-3" /> Copied</>
                      ) : (
                        <><Copy className="w-3 h-3" /> Copy</>
                      )}
                    </button>
                  </div>
                )}
              </div>
            )
          })}

          {busy && (
            <div className="flex items-center gap-2 text-text-secondary text-sm">
              <Loader2 className="w-4 h-4 animate-spin" />
              <span>Working...</span>
              {canCancel && (
                <button
                  onClick={onCancel}
                  className="flex items-center gap-1 ml-2 px-2 py-0.5 rounded text-xs font-medium bg-bad/10 text-bad hover:bg-bad/20 transition-colors"
                >
                  <Square className="w-3 h-3" />
                  Cancel
                </button>
              )}
            </div>
          )}

          {awaitingApproval && approval && (
            <div className="rounded-lg bg-warn-soft border border-warn/30 p-4 space-y-3">
              <div className="flex items-center gap-2">
                <span className="text-sm text-warn font-medium">Approval required</span>
                <span className="text-xs text-text-tertiary">{approval.kind}</span>
              </div>
              <p className="text-sm text-text-primary">{approval.summary}</p>
              {approval.reason && (
                <p className="text-xs text-text-secondary">{approval.reason}</p>
              )}

              {approval.transaction && (
                <div className="mt-2 space-y-2 border-t border-warn/20 pt-2">
                  {approval.transaction.action_summary && (
                    <p className="text-xs font-mono text-text-secondary">{approval.transaction.action_summary}</p>
                  )}
                  {approval.transaction.invoked_programs && approval.transaction.invoked_programs.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {approval.transaction.invoked_programs.map((prog, idx) => (
                        <span key={idx} className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-bg-secondary text-text-secondary">
                          {prog}
                        </span>
                      ))}
                    </div>
                  )}
                  {approval.transaction.lamport_deltas && approval.transaction.lamport_deltas.length > 0 && (
                    <table className="w-full text-xs font-mono">
                      <thead>
                        <tr className="text-text-tertiary">
                          <th className="text-left py-1">Account</th>
                          <th className="text-right py-1">Delta (lamports)</th>
                        </tr>
                      </thead>
                      <tbody>
                        {approval.transaction.lamport_deltas.map((d, idx) => (
                          <tr key={idx} className="text-text-secondary">
                            <td className="py-0.5 truncate max-w-[200px]">{d.account}</td>
                            <td className={cn('py-0.5 text-right', BigInt(d.delta_lamports) < 0 ? 'text-bad' : 'text-ok')}>
                              {d.delta_lamports}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                  {approval.transaction.compute_units_consumed != null && (
                    <p className="text-[10px] text-text-tertiary">
                      Compute: {approval.transaction.compute_units_consumed.toLocaleString()}
                      {approval.transaction.compute_unit_limit != null && ` / ${approval.transaction.compute_unit_limit.toLocaleString()} CU`}
                    </p>
                  )}
                </div>
              )}

              <div className="flex gap-2 pt-1">
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
