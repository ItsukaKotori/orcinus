import { createRoot } from 'react-dom/client'
import '../theme.css'
import { createHostBridge } from '../bridge'
import { PanelApp } from './App'

createRoot(document.getElementById('root') as HTMLElement).render(
  <PanelApp bridge={createHostBridge()} />
)
