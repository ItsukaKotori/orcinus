import { createRoot } from 'react-dom/client'
import '../theme.css'
import { createHostBridge } from '../bridge'
import { TabApp } from './App'

createRoot(document.getElementById('root') as HTMLElement).render(
  <TabApp bridge={createHostBridge()} />
)
