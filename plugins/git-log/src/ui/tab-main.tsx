import { createRoot } from 'react-dom/client'
import './theme.css'
import { GitLogView } from './GitLogView'
import { FixtureRepository } from '../fixtures/fixture-repository'

const el = document.getElementById('git-log-root')
if (el) createRoot(el).render(<GitLogView repository={new FixtureRepository()} />)
