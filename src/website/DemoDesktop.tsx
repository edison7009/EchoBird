import { useState } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  PanelLeft,
  Minus,
  Maximize2,
  X,
  House,
  Box,
  Users,
  Waypoints,
  Mic,
  Newspaper,
  Trophy,
  Server,
  Settings,
  RefreshCw,
} from 'lucide-react';
import { DesktopIcon } from '../components/DesktopToolIcon';
import {
  AccountSection,
  AccountSectionButton,
  AccountSectionRow,
} from '../pages/AppManager/AccountSectionPrimitives';
import { ModelListCard } from '../components/ModelListCard';
import { ViewModeTabs } from '../components/ViewModeTabs';
import { PageActionButton } from '../components/PageActionButton';
import { CapabilityToggles } from '../pages/AppManager/CapabilityToggles';
import { useI18n } from '../hooks/useI18n';
import { desktopCopy } from './demoLocale';
import { demoTools, demoModels, demoAccounts, demoQuotas, demoModelUsage } from './fixtures';
import { QuotaPercent } from '../pages/AppManager/QuotaPercent';
import { QuotaCountdown } from '../pages/AppManager/QuotaCountdown';
import { quotaColorClasses, quotaTone } from '../utils/quotaColors';

function DemoQuota({ email, startedAt }: { email: string; startedAt: number }) {
  const { locale } = useI18n();
  const copy = desktopCopy[locale === 'en' ? 'en' : 'zh-Hans'];
  const quota = demoQuotas[email];
  const colors = quotaColorClasses[quotaTone(quota.percent)];
  return (
    <span className="flex min-w-0 items-center gap-2 whitespace-nowrap" aria-label={copy.quota}>
      <span className="text-[10px] text-cyber-text-secondary">5h</span>
      <QuotaPercent percent={quota.percent} />
      <span
        role="progressbar"
        aria-label={`${email} ${copy.quota}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={quota.percent}
        className={`relative h-0 w-10 shrink-0 rounded-full border-t-[6px] ${colors.track}`}
      >
        <span
          className={`absolute bottom-0 left-0 h-0 rounded-full border-t-[6px] ${colors.fill}`}
          style={{ width: `${quota.percent}%` }}
        />
      </span>
      <QuotaCountdown
        resetAt={Math.floor(startedAt / 1000) + quota.resetMinutes * 60}
        compact
        small
        parenthesized
        label={copy.quotaReset}
      />
    </span>
  );
}

// The website owns only transient demonstration state. Shared components own appearance.
export function DemoDesktop() {
  const { t, locale } = useI18n();
  const copy = desktopCopy[locale === 'en' ? 'en' : 'zh-Hans'];
  const [toolId, setToolId] = useState('claudecode');
  const [view, setView] = useState<'desktop' | 'install'>('desktop');
  const [selection, setSelection] = useState('smart-router-demo');
  const [accounts, setAccounts] = useState(demoAccounts);
  const [models, setModels] = useState(demoModels);
  const [startedAt] = useState(Date.now);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [webSearch, setWebSearch] = useState(true);
  const [oneM, setOneM] = useState(false);
  const tool = demoTools.find((item) => item.id === toolId)!;
  const deletingModel = models.some((model) => model.internalId === deleting);
  return (
    <div className="app-window-shell relative isolate flex h-screen w-full flex-col overflow-hidden rounded-xl bg-cyber-bg text-cyber-text font-sans">
      <div
        aria-hidden="true"
        className="app-background-basalt pointer-events-none absolute inset-0 -z-10"
      />
      <header className="flex h-8 shrink-0 items-center justify-between px-3 text-cyber-text-secondary text-xs">
        <div className="flex items-center gap-4">
          <ArrowLeft size={14} />
          <ArrowRight size={14} />
          <PanelLeft size={14} />
        </div>
        <div className="flex items-center gap-5">
          <button>{copy.updates}</button>
          <button>{copy.feedback}</button>
          <Settings size={14} />
          <img src="/brand/bird.png" className="h-4 w-4" alt="" />
          <Minus size={14} />
          <Maximize2 size={14} />
          <X size={14} />
        </div>
      </header>
      <div className="relative isolate flex min-h-0 flex-1 gap-4 p-4">
        <aside className="flex w-10 shrink-0 flex-col items-center gap-3" aria-label={copy.rail}>
          <img src="/brand/bird.png" alt="EchoBird" className="mb-2 h-8 w-8 object-contain" />
          {[House, Box, Users, Waypoints, Mic, Newspaper, Trophy, Server].map((Icon, index) => (
            <button
              key={index}
              aria-label={copy.pages[index]}
              className={`flex h-10 w-10 items-center justify-center rounded-lg ${index === 0 ? 'bg-cyber-elevated/75' : 'text-cyber-text-secondary'}`}
            >
              <Icon size={20} />
            </button>
          ))}
        </aside>
        <section className="flex min-w-0 flex-1 flex-col -mt-1.5">
          <div className="flex min-h-0 flex-1 gap-6">
            <main className="flex min-w-0 flex-1 flex-col">
              <div className="page-title-bar mb-3 flex h-10 shrink-0 items-center gap-3">
                <h2 className="cjk-title shrink-0">{copy.title}</h2>
                <span className="page-kicker">DESKTOP</span>
                <div className="ml-auto flex items-center gap-3">
                  <PageActionButton icon={Settings}>{copy.customize}</PageActionButton>
                  <PageActionButton icon={RefreshCw}>{copy.refresh}</PageActionButton>
                  <ViewModeTabs
                    value={view}
                    onChange={(value) => {
                      setView(value);
                      setSelection('');
                    }}
                    options={[
                      { value: 'desktop', label: copy.apps },
                      { value: 'install', label: copy.store },
                    ]}
                  />
                </div>
              </div>
              <div className="demo-tool-grid grid grid-cols-5 gap-x-2 gap-y-4 content-start overflow-y-auto">
                {demoTools.map((item) => (
                  <DesktopIcon
                    key={item.id}
                    tool={item}
                    selected={toolId === item.id}
                    onClick={() => {
                      setToolId(item.id);
                      setSelection('');
                    }}
                  />
                ))}
              </div>
            </main>
            <aside
              className="demo-account-panel flex w-[296px] shrink-0 flex-col gap-2"
              aria-label={copy.panel}
            >
              <AccountSection>
                <AccountSectionButton
                  iconSrc={tool.icon}
                  busy={false}
                  remainingSeconds={0}
                  onClick={() => {}}
                  onCancel={() => {}}
                  idleLabel="agent.addCurrentAccount"
                />
                {accounts.map((email, index) => (
                  <AccountSectionRow
                    key={email}
                    email={email}
                    selected={selection === email}
                    plan={index === 0 ? 'Pro' : '—'}
                    secondary={<DemoQuota email={email} startedAt={startedAt} />}
                    onSelect={() => setSelection(email)}
                    onDelete={() => setDeleting(email)}
                  />
                ))}
              </AccountSection>
              <CapabilityToggles
                toggles={[
                  {
                    label: 'Web Search',
                    hint: t('agent.codexWebSearchHint'),
                    checked: webSearch,
                    onChange: setWebSearch,
                  },
                  {
                    label: '1M',
                    hint: t('agent.claude1mHint'),
                    checked: oneM,
                    onChange: setOneM,
                  },
                ]}
              />
              {models.map((model) => (
                <ModelListCard
                  compact
                  key={model.internalId}
                  model={{
                    ...model,
                    name:
                      model.internalId === 'smart-router-demo'
                        ? copy.group
                        : model.internalId === 'smart-router-coding'
                          ? copy.coding
                          : model.name,
                  }}
                  selection={null}
                  selected={selection === model.internalId}
                  onSelect={() => setSelection(model.internalId)}
                  subtitle={
                    model.internalId.startsWith('smart-router') ? copy.route : model.modelId
                  }
                  badge={
                    model.internalId.startsWith('smart-router') ? (
                      <span className="shrink-0 text-[10px] font-medium leading-none text-cyber-accent">
                        {copy.smart}
                      </span>
                    ) : undefined
                  }
                  secondaryBadge={
                    model.internalId.startsWith('smart-router') ? (
                      <span className="model-switch-label shrink-0 text-[10px] font-medium leading-none text-emerald-700">
                        {copy.switchable}
                      </span>
                    ) : undefined
                  }
                  usage={demoModelUsage[model.internalId]}
                  onRefreshUsage={
                    model.internalId.startsWith('smart-router') ? undefined : () => {}
                  }
                  onEditModel={model.internalId.startsWith('smart-router') ? undefined : () => {}}
                  onDeleteModel={
                    model.internalId.startsWith('smart-router') ? undefined : setDeleting
                  }
                  t={t}
                />
              ))}
            </aside>
          </div>
          <footer className="mt-4 flex h-[78px] shrink-0 items-center justify-between gap-6 border-t border-cyber-border">
            <p role="status" className="min-w-0 text-xs text-cyber-accent">
              {copy.status}
            </p>
            <button className="h-12 w-52 shrink-0 rounded-button bg-cyber-accent text-white font-semibold">
              {copy.launch}
            </button>
          </footer>
        </section>
      </div>
      {deleting && (
        <div className="absolute inset-0 z-20 grid place-items-center bg-black/50">
          <section
            role="dialog"
            aria-modal="true"
            aria-label={deletingModel ? copy.deleteModel : copy.deleteAccount}
            className="w-80 rounded-xl border border-cyber-border bg-cyber-surface p-6"
          >
            <h2 className="text-lg font-semibold">
              {deletingModel ? copy.questionModel : copy.questionAccount}
            </h2>
            <p className="my-4 text-sm text-cyber-text-secondary">{copy.deleteBody}</p>
            <div className="flex justify-end gap-3">
              <PageActionButton onClick={() => setDeleting(null)}>{copy.cancel}</PageActionButton>
              <PageActionButton
                onClick={() => {
                  if (deletingModel)
                    setModels(models.filter((model) => model.internalId !== deleting));
                  else setAccounts(accounts.filter((email) => email !== deleting));
                  if (selection === deleting) setSelection('');
                  setDeleting(null);
                }}
              >
                {copy.remove}
              </PageActionButton>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
