// Sidebar navigation component
import { useRef, type ReactNode } from 'react';
import {
  Box,
  Server,
  CodeXml,
  Trophy,
  Newspaper,
  Monitor,
  Waypoints,
  Sparkles,
  RefreshCw,
  Users,
  Mic,
  Download,
  Gauge,
} from 'lucide-react';
import { NavItem } from './NavItem';
import { RoutingToggle } from './RoutingToggle';
import { useToast } from './Toast';
import { useI18n } from '../hooks/useI18n';
import type { TKey } from '../i18n';
import type { SmartRouterConfig } from '../api/smartRouter';
import { useLocalServerStatusStore } from '../stores/localServerStatusStore';
import { useUsageIslandStore } from '../stores/usageIslandStore';
import { useVoiceInputStore, voiceBusy } from '../stores/voiceInputStore';
import { BirdEasterEgg } from './BirdEasterEgg';
import { useSidebarStore } from '../stores/sidebarStore';
import { useNavigationStore } from '../stores/navigationStore';

declare const __APP_EDITION__: string;
const isFullEdition = __APP_EDITION__ === 'full';

export type PageType =
  | 'models'
  | 'accounts'
  | 'freeModels'
  | 'apps'
  | 'aiNews'
  | 'aiCareer'
  | 'voice'
  | 'developer'
  | 'localLlm'
  | 'mother'
  | 'feedback';

interface SidebarProps {
  activePage: PageType;
  onPageChange: (page: PageType) => void;
  agentRunning?: boolean;
  smartRouterGroups: ReadonlyArray<Pick<SmartRouterConfig, 'candidateIds' | 'groupEnabled'>>;
  smartRouterEnabled: boolean;
  smartRouterTogglePending: boolean;
  onSmartRouterChange: (enabled: boolean) => void;
  updateAvailable?: string | null;
  onSettingsClick?: () => void;
}

function SidebarToggle({
  collapsed,
  label,
  ariaLabel = label,
  icon,
  checked,
  disabled = false,
  pending = false,
  onChange,
}: {
  collapsed: boolean;
  label: string;
  ariaLabel?: string;
  icon: ReactNode;
  checked: boolean;
  disabled?: boolean;
  pending?: boolean;
  onChange: (enabled: boolean) => void;
}) {
  if (collapsed)
    return (
      <button
        type="button"
        role="switch"
        aria-label={ariaLabel}
        aria-checked={checked}
        aria-busy={pending}
        disabled={disabled || pending}
        className={`mx-auto flex h-10 w-10 shrink-0 items-center justify-center rounded-lg transition-colors ${disabled ? 'disabled:opacity-40' : ''} ${checked ? 'bg-cyber-accent/15 text-cyber-accent' : 'text-cyber-text-secondary hover:bg-cyber-elevated/50 hover:text-cyber-text'}`}
        onClick={() => {
          if (!disabled && !pending) return onChange(!checked);
        }}
      >
        {icon}
      </button>
    );
  return (
    <div className="flex items-center gap-4 px-2 py-1">
      <span className="whitespace-nowrap">{label}</span>
      <RoutingToggle
        label={ariaLabel}
        hideLabel
        checked={checked}
        disabled={disabled}
        pending={pending}
        onChange={onChange}
      />
    </div>
  );
}

export const Sidebar = ({
  activePage,
  onPageChange,
  agentRunning = false,
  smartRouterGroups,
  smartRouterEnabled,
  smartRouterTogglePending,
  onSmartRouterChange,
  updateAvailable = null,
  onSettingsClick,
}: SidebarProps) => {
  const { t } = useI18n();
  const { showToast } = useToast();
  const collapsed = useSidebarStore((state) => state.collapsed);
  const developerMode = useNavigationStore((state) => state.developerMode);
  const sidebarRef = useRef<HTMLElement>(null);
  const perchRef = useRef<HTMLDivElement>(null);
  const serverRunning = useLocalServerStatusStore((state) => state.running);
  const voiceEnabled = useVoiceInputStore(
    (state) => state.snapshot?.config.shortcutEnabled ?? false
  );
  const hasVoiceChannels = useVoiceInputStore(
    (state) => (state.snapshot?.config.channels.length ?? 0) > 0
  );
  const voicePending = useVoiceInputStore((state) => state.saving || state.pending);
  const voiceDisabled = useVoiceInputStore(
    (state) => !state.snapshot || voiceBusy(state.snapshot.phase)
  );
  const saveVoice = useVoiceInputStore((state) => state.save);
  const hasUsageSources = useUsageIslandStore((state) => state.sources.length > 0);
  const usageIslandEnabled = useUsageIslandStore((state) => state.enabled);
  const updateUsageIsland = useUsageIslandStore((state) => state.update);
  const showSmartRouterToggle = smartRouterGroups.some((router) => router.candidateIds.length > 0);
  const changeVoice = async (enabled: boolean) => {
    if (voiceDisabled || voicePending) return;
    if (await saveVoice({ shortcutEnabled: enabled })) return;
    const error = useVoiceInputStore.getState().error;
    if (error) showToast('error', error.startsWith('voice.') ? t(error as TKey) : error);
  };

  return (
    <nav
      id="app-sidebar"
      ref={sidebarRef}
      data-collapsed={collapsed}
      className={`relative flex shrink-0 flex-col pb-5 transition-[width,padding,margin] duration-200 ease-out motion-reduce:transition-none ${collapsed ? 'w-16 -ml-4 px-2' : 'w-64 px-6'}`}
    >
      <div
        className={`mb-7 flex-shrink-0 flex items-center ${collapsed ? 'relative justify-center' : 'gap-2 overflow-hidden'}`}
      >
        <BirdEasterEgg sidebarRef={sidebarRef} perchRef={perchRef} />
        {!collapsed && (
          <span className="brand-mark flex-shrink-0 text-cyber-text">{t('app.name')}</span>
        )}
        {updateAvailable && !collapsed && (
          <button
            type="button"
            onClick={onSettingsClick}
            aria-label={t('settings.updates')}
            className="flex-shrink-0 text-[11px] font-mono text-red-400 hover:opacity-70 transition-opacity animate-pulse leading-none"
          >
            {t('settings.updates')}
          </button>
        )}
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto page-scroll space-y-5 text-[15px]">
        <NavItem
          collapsed={collapsed}
          icon={<Monitor size={20} />}
          label={t('nav.appManager')}
          active={activePage === 'apps'}
          onClick={() => onPageChange('apps')}
        />
        <NavItem
          collapsed={collapsed}
          icon={<Box size={20} />}
          label={t('nav.modelNexus')}
          active={activePage === 'models'}
          onClick={() => onPageChange('models')}
        />
        <NavItem
          collapsed={collapsed}
          icon={<Users size={20} />}
          label={t('nav.accountCenter')}
          active={activePage === 'accounts'}
          onClick={() => onPageChange('accounts')}
        />
        <NavItem
          collapsed={collapsed}
          icon={<Waypoints size={20} />}
          label={t('nav.freeModels')}
          active={activePage === 'freeModels'}
          onClick={() => onPageChange('freeModels')}
        />
        <NavItem
          collapsed={collapsed}
          icon={<Mic size={20} />}
          id="voice-input-nav"
          label={t('voice.input')}
          active={activePage === 'voice'}
          onClick={() => onPageChange('voice')}
        />
        <NavItem
          collapsed={collapsed}
          icon={
            collapsed && agentRunning ? (
              <span
                aria-hidden="true"
                className="absolute inset-0 flex items-center justify-center rounded-lg bg-cyber-accent/15 text-cyber-accent"
              >
                <RefreshCw size={20} className="animate-spin" />
              </span>
            ) : (
              <Sparkles size={20} />
            )
          }
          label={t('nav.motherAgent')}
          active={activePage === 'mother'}
          onClick={() => onPageChange('mother')}
          trailing={
            !collapsed &&
            agentRunning && (
              <RefreshCw size={14} aria-hidden="true" className="animate-spin text-cyber-accent" />
            )
          }
        />
        {/* Divider — primary actions above; news, career and project tools below */}
        <div ref={perchRef} className="border-t border-cyber-border/50" />
        <NavItem
          collapsed={collapsed}
          icon={<Newspaper size={20} />}
          label={t('nav.aiNews')}
          active={activePage === 'aiNews'}
          onClick={() => onPageChange('aiNews')}
        />
        <NavItem
          collapsed={collapsed}
          icon={<Trophy size={20} />}
          label={t('nav.aiCareer')}
          active={activePage === 'aiCareer'}
          onClick={() => onPageChange('aiCareer')}
        />
        {developerMode && (
          <NavItem
            collapsed={collapsed}
            icon={<CodeXml size={20} />}
            label={t('nav.developer')}
            active={activePage === 'developer'}
            onClick={() => onPageChange('developer')}
          />
        )}
        {isFullEdition && (
          <NavItem
            collapsed={collapsed}
            icon={<Server size={20} />}
            label={`${t('nav.localServer')}${collapsed && serverRunning ? `: ${t('status.running')}` : ''}`}
            active={activePage === 'localLlm'}
            onClick={() => onPageChange('localLlm')}
            trailing={
              collapsed &&
              serverRunning && (
                <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-cyber-accent" />
              )
            }
          />
        )}
      </div>

      <div className="flex-shrink-0 pt-4 text-[14px] text-cyber-text-secondary">
        <div className="space-y-1 text-[15px] font-medium">
          {collapsed && updateAvailable && (
            <button
              type="button"
              aria-label={t('settings.updates')}
              onClick={onSettingsClick}
              className="mx-auto flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-red-400 transition-colors hover:bg-cyber-elevated/50"
            >
              <Download
                size={20}
                aria-hidden="true"
                className="animate-pulse motion-reduce:animate-none"
              />
            </button>
          )}
          {hasVoiceChannels && (
            <SidebarToggle
              collapsed={collapsed}
              label={t('voice.input')}
              ariaLabel={t('voice.enableInput')}
              icon={<Mic size={20} />}
              checked={voiceEnabled}
              disabled={voiceDisabled}
              pending={voicePending}
              onChange={changeVoice}
            />
          )}
          {developerMode && hasUsageSources && (
            <SidebarToggle
              collapsed={collapsed}
              label={t('developer.usageIsland')}
              ariaLabel={t('usageIsland.enable')}
              icon={<Gauge size={20} />}
              checked={usageIslandEnabled}
              onChange={(enabled) => updateUsageIsland({ enabled })}
            />
          )}
          {showSmartRouterToggle && (
            <SidebarToggle
              collapsed={collapsed}
              label={t('nav.smartRouter')}
              icon={<Waypoints size={20} />}
              checked={smartRouterEnabled}
              pending={smartRouterTogglePending}
              onChange={onSmartRouterChange}
            />
          )}
        </div>
        {isFullEdition && serverRunning && !collapsed && (
          <div className="mt-1">
            {t('nav.localServer')}:{' '}
            <span className="text-cyber-accent font-semibold">{t('status.running')}</span>
          </div>
        )}
      </div>
    </nav>
  );
};
