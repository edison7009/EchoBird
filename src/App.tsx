// App.tsx — Tauri application shell (lightweight router)
// Layout matches the original v1.1.0 structure exactly.
// Pages extracted to src/pages/ with Provider pattern.

import { useState, useEffect, useCallback, useMemo } from 'react';
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
        };
        preload();
    }, []);

    // Clear logs
    const onClearLogs = useCallback(() => setAppLogs([]), []);

    // ──────────────────────────────────────────────
    // Wrap ALL page content in providers.
    // We render providers at the top level so that both
    // the <h2> title actions AND the <aside> panel share
    // the same context.
    // ──────────────────────────────────────────────

    const renderProviderStart = () => {
        // MotherAgent is always mounted (hidden via CSS), so its provider must always wrap content
        const withMotherAgent = (content: React.ReactNode) => (
            <MotherAgentProvider appLogs={appLogs} detectedTools={detectedTools} onClearLogs={onClearLogs} onAgentRunningChange={setAgentRunning}>
                {content}
            </MotherAgentProvider>
        );

        switch (activePage) {
            case 'models': return withMotherAgent(<ModelNexusProvider>{innerContent}<AddModelModal /></ModelNexusProvider>);
            case 'skills': return withMotherAgent(<SkillBrowserProvider preloadedSkills={preloadedSkills}>{innerContent}</SkillBrowserProvider>);
            case 'apps': return withMotherAgent(
                <AppManagerProvider detectedTools={detectedTools} setDetectedTools={setDetectedTools} isScanning={isScanning} scanTools={doScanTools} modelProtocolSelection={modelProtocolSelection} setModelProtocolSelection={setModelProtocolSelection}>
                    {innerContent}
                    <AppManagerErrorModal />
                </AppManagerProvider>
            );
            case 'localLlm': return withMotherAgent(<LocalServerProvider>{innerContent}</LocalServerProvider>);
            case 'mother': return withMotherAgent(innerContent);
            default: return withMotherAgent(innerContent);
        }
    };

    const innerContent = useMemo(() => (
        <>
            {/* Main + Right panel row — matches original v1.1.0 structure */}
            <div className="flex-1 flex gap-3 overflow-hidden">
                <main className="flex-1 flex flex-col overflow-hidden">
                    <section className="flex-1 flex flex-col overflow-hidden pr-2">
                        {/* Shared page title bar */}
                        <h2 className="text-xl mb-3 flex-shrink-0 relative flex items-center cjk-title">
                            <span className="truncate">
                                {activePage === 'models' && t('page.modelNexus')}
                                {activePage === 'skills' && <span className="text-cyber-warning">{t('page.skillBrowser')}</span>}
                                {activePage === 'apps' && t('page.appManager')}
                                {activePage === 'localLlm' && t('page.localServer')}
                                {activePage === 'mother' && <span className="text-cyber-accent-secondary">{t('page.motherAgent')}</span>}
                                {activePage === 'channels' && t('page.channels')}
                            </span>
                            {activePage === 'skills' && <SkillBrowserTranslateList />}
                            {activePage === 'models' && <ModelNexusTitleActions />}
                            {activePage === 'skills' && <SkillBrowserSearch />}
                            {activePage === 'mother' && (
                                <div className="ml-auto flex-shrink-0">
                                    <MotherAgentModelSelector />
                                </div>
                            )}
                        </h2>

                        {/* Page content — each page needs different overflow behavior */}
                        {activePage === 'models' && (
                            <div className="flex-1 overflow-y-auto"><ModelNexusMain /></div>
                        )}
                        {activePage === 'skills' && (
                            <div className="flex flex-col flex-1 overflow-hidden"><SkillBrowserMain /></div>
                        )}
                        {activePage === 'apps' && (
                            <div className="flex-1 flex flex-col overflow-hidden">
                                <AppManagerMain />
                            </div>
                        )}
                        {activePage === 'localLlm' && (
                            <div className="flex-1 flex flex-col overflow-hidden"><LocalServerMain /></div>
                        )}

                        {activePage === 'channels' && (
                            <div className="flex-1 overflow-y-auto"><Channels /></div>
                        )}
                        {/* MotherAgent: always mounted, hidden via CSS to preserve chat state */}
                        <div className={`flex-1 flex flex-col overflow-hidden ${activePage !== 'mother' ? 'hidden' : ''}`}>
                            <MotherAgentMain />
                        </div>
                    </section>
                </main>

                {/* Right panel (hidden on channels page) */}
                {activePage !== 'channels' && (
                    <aside className="w-80 flex flex-col">
                        {activePage === 'models' && <ModelNexusPanel />}
                        {activePage === 'skills' && <SkillBrowserPanel />}
                        {activePage === 'apps' && <AppManagerPanel />}
                        {activePage === 'localLlm' && <LocalServerPanel />}
                        {/* MotherAgent panel: always mounted, hidden via CSS */}
                        <div className={activePage !== 'mother' ? 'hidden' : 'contents'}>
                            <MotherAgentPanel />
                        </div>
                    </aside>
                )}
            </div>

            {/* App Manager bottom bar — spans full width across main + aside */}
            {activePage === 'apps' && <AppManagerBottom />}
            {activePage === 'localLlm' && <LocalServerBottom />}

            {/* Bottom bar */}
            <div className="flex-shrink-0 pt-2">
                <DownloadBar />
            </div>
        </>
    ), [activePage, t]);

    return (
        <ToastProvider>
            <ConfirmDialogProvider>
                <DownloadProvider>
                    <GatewayProvider>
                        <div className="flex flex-col h-screen w-full bg-cyber-bg">
                            {/* Title bar */}
                            <TitleBar onSettingsClick={() => setShowSettings(true)} />
                            <div className="flex flex-1 overflow-hidden text-cyber-accent font-mono p-4 gap-4 grid-bg relative isolate">
                                <CircuitFlowConnected />
                                {/* Sidebar */}
                                <Sidebar activePage={activePage} onPageChange={setActivePage} agentRunning={agentRunning} />

                                {/* Main content wrapper */}
                                <div className="flex-1 flex flex-col overflow-hidden">
                                    {renderProviderStart()}
                                </div>
                            </div>
                        </div>

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
        </ToastProvider>
    );
}

export default App;
