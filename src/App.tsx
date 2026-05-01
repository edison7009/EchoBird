// App.tsx — Tauri application shell (lightweight router)
// Layout matches the original v1.1.0 structure exactly.
// Pages extracted to src/pages/ with Provider pattern.
// All Providers are always mounted; pages are shown/hidden via CSS to avoid remounting.

import { useState, useEffect } from 'react';
import { RotateCcw } from 'lucide-react';
import { Sidebar, PageType, ToastProvider, ConfirmDialogProvider } from './components';
import { DownloadProvider } from './components/DownloadContext';
import { DownloadBar } from './components/DownloadBar';
import { TitleBar } from './components/TitleBar';
import { SettingsDialog } from './components/SettingsDialog';
import { CircuitFlow } from './components/CircuitFlow';

import { useI18n } from './hooks/useI18n';
import * as api from './api/tauri';

// Zustand stores
import { useToolsStore } from './stores/toolsStore';
import { useNavigationStore } from './stores/navigationStore';

// Pages
import { ModelNexusProvider, ModelNexusTitleActions, ModelNexusMain, ModelNexusPanel, AddModelModal } from './pages/ModelNexus';

import { AppManagerProvider, AppManagerMain, AppManagerPanel, AppManagerBottom, AppManagerErrorModal } from './pages/AppManager';
import { LocalServerProvider, LocalServerMain, LocalServerPanel, LocalServerBottom } from './pages/LocalServer';
import { MotherAgentProvider, MotherAgentMain, MotherAgentPanel } from './pages/MotherAgent';

declare const __APP_VERSION__: string;


function SidebarConnected({ onSettingsClick }: { onSettingsClick: () => void }) {
    const { activePage, setActivePage, agentRunning, motherNewMessage, clearMotherBadge, updateAvailable } = useNavigationStore();
    const motherBadge = motherNewMessage && activePage !== 'mother';
    // Clear badge when switching to Mother Agent page
    const handlePageChange = (p: PageType) => {
        if (p === 'mother') clearMotherBadge();
        setActivePage(p);
    };
    return <Sidebar activePage={activePage} onPageChange={handlePageChange} agentRunning={agentRunning} motherBadge={motherBadge} updateAvailable={updateAvailable} onSettingsClick={onSettingsClick} />;
}

// Helper: h (hidden) vs shown class
const page = (active: boolean) => active ? 'contents' : 'hidden';
const pageBlock = (active: boolean) => active ? 'flex-1 flex flex-col overflow-hidden' : 'hidden';
const pageScroll = (active: boolean) => active ? 'flex-1 overflow-y-auto' : 'hidden';

function App() {
    const { t, locale, setLocale } = useI18n();
    const [showSettings, setShowSettings] = useState(false);

    // Stores
    const { activePage, flashCount, setUpdateAvailable } = useNavigationStore();
    const scanTools = useToolsStore(s => s.scanTools);

    // ── Splash preload: run scanTools then mark app ready.
    useEffect(() => {
        const preload = async () => {
            await scanTools();
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

    const is = (p: PageType) => activePage === p;

    return (
        <ToastProvider>
            <ConfirmDialogProvider>
                <DownloadProvider>
                    {/* All Providers always mounted — only CSS hidden changes */}
                        <MotherAgentProvider>
                            <ModelNexusProvider>

                                    <AppManagerProvider>
                                        <LocalServerProvider>

                                            <div className="flex flex-col h-screen w-full bg-cyber-bg">
                                                {/* Title bar */}
                                                <TitleBar onSettingsClick={() => setShowSettings(true)} />
                                                <div className="flex flex-1 overflow-hidden text-cyber-accent font-mono p-4 gap-0 grid-bg relative isolate">
                                                    <CircuitFlow flashCount={flashCount} />
                                                    {/* Sidebar */}
                                                    <SidebarConnected onSettingsClick={() => setShowSettings(true)} />

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

                                                                            {is('apps') && t('page.appManager')}
                                                                            {is('localLlm') && t('page.localServer')}
                                                                            {is('mother') && t('page.motherAgent')}
                                                                        </span>
                                                                        {/* Title actions — always mounted but hidden */}

                                                                        <span className={page(is('models'))}><ModelNexusTitleActions /></span>

                                                                        {is('mother') && (
                                                                            <div className="ml-auto flex-shrink-0 flex items-center gap-2">
                                                                                <button
                                                                                    onClick={() => window.dispatchEvent(new CustomEvent('clear-chat'))}
                                                                                    className="p-1.5 rounded-lg text-cyber-accent/40 hover:text-cyber-accent hover:bg-cyber-accent/10 transition-colors"
                                                                                >
                                                                                    <RotateCcw size={14} />
                                                                                </button>
                                                                            </div>
                                                                        )}
                                                                    </h2>

                                                                    {/* Page content — always mounted, CSS hidden */}
                                                                    <div className={pageScroll(is('models'))}><ModelNexusMain /></div>

                                                                    <div className={pageBlock(is('apps'))}><AppManagerMain /></div>
                                                                    <div className={pageBlock(is('localLlm'))}><LocalServerMain /></div>
                                                                    {/* MotherAgent: always mounted, hidden via CSS to preserve chat state */}
                                                                    <div className={`flex-1 flex flex-col overflow-hidden ${is('mother') ? '' : 'hidden'}`}>
                                                                        <MotherAgentMain />
                                                                    </div>

                                                                </section>
                                                            </main>

                                                            <aside className="w-80 flex flex-col">
                                                                    <div className={page(is('models'))}><ModelNexusPanel /></div>

                                                                    <div className={page(is('apps'))}><AppManagerPanel /></div>
                                                                    <div className={page(is('localLlm'))}><LocalServerPanel /></div>
                                                                    {/* MotherAgent panel: always mounted, hidden via CSS */}
                                                                    <div className={!is('mother') ? 'hidden' : 'contents'}>
                                                                        <MotherAgentPanel />
                                                                    </div>
                                                                </aside>
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

                            </ModelNexusProvider>
                        </MotherAgentProvider>

                        {/* Settings dialog */}
                        <SettingsDialog
                            isOpen={showSettings}
                            onClose={() => setShowSettings(false)}
                            locale={locale}
                            onLocaleChange={setLocale}
                        />

                </DownloadProvider>
            </ConfirmDialogProvider>
        </ToastProvider >
    );
}

export default App;
