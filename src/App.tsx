// App.tsx — Tauri application shell (lightweight router)
// Layout matches the original v1.1.0 structure exactly.
// Pages extracted to src/pages/ with Provider pattern.
// All Providers are always mounted; pages are shown/hidden via CSS to avoid remounting.

import { useState, useEffect, useCallback } from 'react';
import { Sidebar, PageType, ToastProvider, ConfirmDialogProvider } from './components';
import { DownloadProvider } from './components/DownloadContext';
import { DownloadBar } from './components/DownloadBar';
import { TitleBar } from './components/TitleBar';
import { SettingsDialog } from './components/SettingsDialog';
import { CircuitFlow } from './components/CircuitFlow';
import { GatewayProvider, useGatewayManager } from './contexts/GatewayContext';
import { useI18n } from './hooks/useI18n';
import * as api from './api/tauri';
import type { SkillsData } from './api/tauri';
import type { LocalTool, AppLogEntry } from './api/types';

// Pages
import { ModelNexusProvider, ModelNexusTitleActions, ModelNexusMain, ModelNexusPanel, AddModelModal } from './pages/ModelNexus';
import { SkillBrowserProvider, SkillBrowserSearch, SkillBrowserMain, SkillBrowserPanel, SkillBrowserTranslateList } from './pages/SkillBrowser';
import { AppManagerProvider, AppManagerMain, AppManagerPanel, AppManagerBottom, AppManagerErrorModal } from './pages/AppManager';
import { LocalServerProvider, LocalServerMain, LocalServerPanel, LocalServerBottom } from './pages/LocalServer';
import { MotherAgentProvider, MotherAgentMain, MotherAgentPanel, MotherAgentModelSelector } from './pages/MotherAgent';
import { Channels } from './pages/Channels';

declare const __APP_VERSION__: string;

/** Reads channel statuses from GatewayManager → drives CircuitFlow pulse colors */
function CircuitFlowConnected() {
    const gw = useGatewayManager();
    const pulses = gw.getChannelPulses();
    return <CircuitFlow channels={pulses.length > 0 ? pulses : undefined} />;
}

/** Sidebar with live notification badges from GatewayContext */
function SidebarConnected({ activePage, onPageChange, agentRunning, motherNewMessage, clearMotherBadge, updateAvailable, onSettingsClick }: { activePage: PageType; onPageChange: (p: PageType) => void; agentRunning: boolean; motherNewMessage: boolean; clearMotherBadge: () => void; updateAvailable: string | null; onSettingsClick: () => void }) {
    const gw = useGatewayManager();
    const channelsBadge = gw.hasAnyNewMessage() && activePage !== 'channels';
    const motherBadge = motherNewMessage && activePage !== 'mother';
    // Clear badge when switching to Mother Agent page
    const handlePageChange = (p: PageType) => {
        if (p === 'mother') clearMotherBadge();
        onPageChange(p);
    };
    return <Sidebar activePage={activePage} onPageChange={handlePageChange} agentRunning={agentRunning} channelsBadge={channelsBadge} motherBadge={motherBadge} updateAvailable={updateAvailable} onSettingsClick={onSettingsClick} />;
}

// Helper: h (hidden) vs shown class
const page = (active: boolean) => active ? 'contents' : 'hidden';
const pageBlock = (active: boolean) => active ? 'flex-1 flex flex-col overflow-hidden' : 'hidden';
const pageScroll = (active: boolean) => active ? 'flex-1 overflow-y-auto' : 'hidden';

function App() {
    const { t, locale, setLocale } = useI18n();
    const [activePage, setActivePage] = useState<PageType>('models');
    const [showSettings, setShowSettings] = useState(false);

    // App logs (shared with MotherAgent)
    const [appLogs, setAppLogs] = useState<AppLogEntry[]>([]);

    // Detected tools (shared with App Manager & MotherAgent)
    const [detectedTools, setDetectedTools] = useState<LocalTool[]>([]);
    const [isScanning, setIsScanning] = useState(false);
    const [modelProtocolSelection, setModelProtocolSelection] = useState<Record<string, 'openai' | 'anthropic'>>({});

    // Preloaded skills data (loaded during splash)
    const [preloadedSkills, setPreloadedSkills] = useState<SkillsData | null>(null);

    // Mother Agent running state
    const [agentRunning, setAgentRunning] = useState(false);
    // Mother Agent new message badge
    const [motherNewMessage, setMotherNewMessage] = useState(false);
    // Pre-fill message for Mother Agent (set when navigating from App Manager install)
    const [motherPrefill, setMotherPrefill] = useState<string | undefined>(undefined);
    // Update available (null = none, string = new version number)
    const [updateAvailable, setUpdateAvailable] = useState<string | null>(null);

    // Scan tools
    const doScanTools = useCallback(async () => {
        setIsScanning(true);
        try {
            const tools = await api.scanTools();
            setDetectedTools(tools);
        } catch { /* ignore */ }
        setIsScanning(false);
    }, []);

    // ── Splash preload: run scanTools + loadSkillsData in parallel,
    //    then close splash only after both finish.
    useEffect(() => {
        const preload = async () => {
            await Promise.all([
                doScanTools(),
                api.loadSkillsData()
                    .then(data => { if (data.skills?.length > 0) setPreloadedSkills(data); })
                    .catch(() => { /* cache miss is OK */ }),
            ]);
            api.appReady();
            // Silent update check after app is ready
            try {
                const res = await fetch('https://echobird.ai/api/version/index.json');
                if (res.ok) {
                    const data = await res.json();
                    if (data.version && data.version !== __APP_VERSION__) {
                        setUpdateAvailable(data.version);
                    }
                }
            } catch { /* network error — ignore silently */ }
        };
        preload();
    }, []);

    // Clear logs
    const onClearLogs = useCallback(() => setAppLogs([]), []);

    const is = (p: PageType) => activePage === p;

    return (
        <ToastProvider>
            <ConfirmDialogProvider>
                <DownloadProvider>
                    <GatewayProvider>
                        {/* All Providers always mounted — only CSS hidden changes */}
                        <MotherAgentProvider appLogs={appLogs} detectedTools={detectedTools} onClearLogs={onClearLogs} onAgentRunningChange={setAgentRunning} onNewMessage={() => setMotherNewMessage(true)} initialMessage={motherPrefill}>
                            <ModelNexusProvider>
                                <SkillBrowserProvider preloadedSkills={preloadedSkills}>
                                    <AppManagerProvider detectedTools={detectedTools} setDetectedTools={setDetectedTools} isScanning={isScanning} scanTools={doScanTools} modelProtocolSelection={modelProtocolSelection} setModelProtocolSelection={setModelProtocolSelection} isActive={activePage === 'apps'} onGoToMother={(toolName) => { setMotherPrefill(t('mother.hintInstall').replace('{agent}', toolName)); setActivePage('mother'); }}>
                                        <LocalServerProvider>

                                            <div className="flex flex-col h-screen w-full bg-cyber-bg">
                                                {/* Title bar */}
                                                <TitleBar onSettingsClick={() => setShowSettings(true)} />
                                                <div className="flex flex-1 overflow-hidden text-cyber-accent font-mono p-4 gap-4 grid-bg relative isolate">
                                                    <CircuitFlowConnected />
                                                    {/* Sidebar */}
                                                    <SidebarConnected activePage={activePage} onPageChange={setActivePage} agentRunning={agentRunning} motherNewMessage={motherNewMessage} clearMotherBadge={() => setMotherNewMessage(false)} updateAvailable={updateAvailable} onSettingsClick={() => setShowSettings(true)} />

                                                    {/* Main content wrapper */}
                                                    <div className="flex-1 flex flex-col overflow-hidden">

                                                        {/* Main + Right panel row */}
                                                        <div className="flex-1 flex gap-3 overflow-hidden">
                                                            <main className="flex-1 flex flex-col overflow-hidden">
                                                                <section className="flex-1 flex flex-col overflow-hidden pr-2">

                                                                    {/* Shared page title bar */}
                                                                    <h2 className="text-xl mb-3 flex-shrink-0 relative flex items-center cjk-title">
                                                                        <span className="truncate">
                                                                            {is('models') && t('page.modelNexus')}
                                                                            {is('skills') && <span className="text-cyber-warning">{t('page.skillBrowser')}</span>}
                                                                            {is('apps') && t('page.appManager')}
                                                                            {is('localLlm') && t('page.localServer')}
                                                                            {is('mother') && <span className="text-cyber-accent-secondary">{t('page.motherAgent')}</span>}
                                                                            {is('channels') && t('page.channels')}
                                                                        </span>
                                                                        {/* Title actions — always mounted but hidden */}
                                                                        <span className={page(is('skills'))}><SkillBrowserTranslateList /></span>
                                                                        <span className={page(is('models'))}><ModelNexusTitleActions /></span>
                                                                        <span className={page(is('skills'))}><SkillBrowserSearch /></span>
                                                                        {is('mother') && (
                                                                            <div className="ml-auto flex-shrink-0">
                                                                                <MotherAgentModelSelector />
                                                                            </div>
                                                                        )}
                                                                    </h2>

                                                                    {/* Page content — always mounted, CSS hidden */}
                                                                    <div className={pageScroll(is('models'))}><ModelNexusMain /></div>
                                                                    <div className={pageBlock(is('skills'))}><SkillBrowserMain /></div>
                                                                    <div className={pageBlock(is('apps'))}><AppManagerMain /></div>
                                                                    <div className={pageBlock(is('localLlm'))}><LocalServerMain /></div>
                                                                    <div className={pageScroll(is('channels'))}><Channels /></div>
                                                                    {/* MotherAgent: always mounted, hidden via CSS to preserve chat state */}
                                                                    <div className={`flex-1 flex flex-col overflow-hidden ${is('mother') ? '' : 'hidden'}`}>
                                                                        <MotherAgentMain />
                                                                    </div>

                                                                </section>
                                                            </main>

                                                            {/* Right panel (hidden on channels page) */}
                                                            {!is('channels') && (
                                                                <aside className="w-80 flex flex-col">
                                                                    <div className={page(is('models'))}><ModelNexusPanel /></div>
                                                                    <div className={page(is('skills'))}><SkillBrowserPanel /></div>
                                                                    <div className={page(is('apps'))}><AppManagerPanel /></div>
                                                                    <div className={page(is('localLlm'))}><LocalServerPanel /></div>
                                                                    {/* MotherAgent panel: always mounted, hidden via CSS */}
                                                                    <div className={!is('mother') ? 'hidden' : 'contents'}>
                                                                        <MotherAgentPanel />
                                                                    </div>
                                                                </aside>
                                                            )}
                                                        </div>

                                                        {/* Bottom bars — always mounted, CSS hidden */}
                                                        <div className={page(is('apps'))}><AppManagerBottom /></div>
                                                        <div className={page(is('localLlm'))}><LocalServerBottom /></div>

                                                        {/* Download bar */}
                                                        <div className="flex-shrink-0 pt-2">
                                                            <DownloadBar />
                                                        </div>
                                                    </div>
                                                </div>
                                            </div>

                                            {/* Modals */}
                                            <AddModelModal />
                                            <AppManagerErrorModal />

                                        </LocalServerProvider>
                                    </AppManagerProvider>
                                </SkillBrowserProvider>
                            </ModelNexusProvider>
                        </MotherAgentProvider>

                        {/* Settings dialog */}
                        <SettingsDialog
                            isOpen={showSettings}
                            onClose={() => setShowSettings(false)}
                            locale={locale}
                            onLocaleChange={setLocale}
                        />
                    </GatewayProvider>
                </DownloadProvider>
            </ConfirmDialogProvider>
        </ToastProvider >
    );
}

export default App;
