import { Tabs, TabsList, TabsTrigger } from '../ui/tabs'

export type PluginCenterTab = 'installed' | 'market' | 'dev'

const PLUGIN_CENTER_TABS: { value: PluginCenterTab; label: string }[] = [
  { value: 'installed', label: '已安装' },
  { value: 'market', label: '市场' },
  { value: 'dev', label: '开发中' }
]

function isPluginCenterTab(value: string): value is PluginCenterTab {
  return PLUGIN_CENTER_TABS.some((tab) => tab.value === value)
}

export function PluginCenterTabs({
  value,
  onValueChange
}: {
  value: PluginCenterTab
  onValueChange: (value: PluginCenterTab) => void
}): React.JSX.Element {
  return (
    <Tabs
      value={value}
      onValueChange={(next) => {
        // Radix hands back a plain string; keep the state union narrow without a cast.
        if (isPluginCenterTab(next)) {
          onValueChange(next)
        }
      }}
    >
      <TabsList>
        {PLUGIN_CENTER_TABS.map((tab) => (
          <TabsTrigger key={tab.value} value={tab.value}>
            {tab.label}
          </TabsTrigger>
        ))}
      </TabsList>
    </Tabs>
  )
}
