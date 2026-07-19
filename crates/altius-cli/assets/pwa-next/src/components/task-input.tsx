'use client'

import { useState, useRef, useEffect } from 'react'
import { Send } from 'lucide-react'

interface TaskInputProps {
  onSubmit: (prompt: string) => void
  disabled: boolean
}

export function TaskInput({ onSubmit, disabled }: TaskInputProps) {
  const [value, setValue] = useState('')
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto'
      textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 120)}px`
    }
  }, [value])

  const handleSubmit = () => {
    if (!value.trim() || disabled) return
    onSubmit(value)
    setValue('')
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSubmit()
    }
  }

  return (
    <div className="border-t border-border-subtle bg-bg-elevated p-4">
      <div className="max-w-3xl mx-auto flex items-end gap-2">
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={disabled}
          placeholder="Send a message to the fleet..."
          rows={1}
          className="flex-1 resize-none rounded-md border border-border-subtle bg-bg-primary px-3 py-2 text-sm text-text-primary placeholder:text-text-tertiary focus:outline-none focus:border-accent transition-colors disabled:opacity-50"
        />
        <button
          onClick={handleSubmit}
          disabled={disabled || !value.trim()}
          className="flex items-center justify-center w-9 h-9 rounded-md bg-accent text-white hover:opacity-90 transition-opacity disabled:opacity-40 flex-shrink-0"
        >
          <Send className="w-4 h-4" />
        </button>
      </div>
    </div>
  )
}
