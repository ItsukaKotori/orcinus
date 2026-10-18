import { createRoot } from 'react-dom/client'
import './theme.css'
import { GitLogView } from './GitLogView'
import { HostRepository } from '../domain/host-repository'

const el = document.getElementById('git-log-root')
if (el) createRoot(el).render(<GitLogView repository={new HostRepository()} />)
