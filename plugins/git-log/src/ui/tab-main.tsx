import { createRoot } from 'react-dom/client'
import './theme.css'

function Placeholder() {
  return <div className="p-4 text-foreground">Git Log (loading scaffold)</div>
}

const el = document.getElementById('git-log-root')
if (el) createRoot(el).render(<Placeholder />)
