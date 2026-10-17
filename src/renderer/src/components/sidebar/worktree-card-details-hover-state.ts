import { useCallback, useRef, useState } from 'react'

export function useWorktreeCardDetailsHoverControl() {
  const [open, setOpen] = useState(false)
  const pendingHoverCloseRef = useRef(false)

  const closeHover = useCallback(() => {
    pendingHoverCloseRef.current = false
    setOpen(false)
  }, [])

  const handleHoverOpenChange = useCallback((next: boolean) => {
    pendingHoverCloseRef.current = false
    setOpen(next)
  }, [])

  return {
    hoverOpen: open,
    handleHoverOpenChange,
    closeHover
  }
}

export type WorktreeCardDetailsHoverControl = ReturnType<typeof useWorktreeCardDetailsHoverControl>
