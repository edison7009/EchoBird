// App.tsx — Tauri application shell (lightweight router)
// Layout matches the original v1.1.0 structure exactly.
// Pages extracted to src/pages/ with Provider pattern.
// All Providers are always mounted; pages are shown/hidden via CSS to avoid remounting.

import { useState, useEffect } from 'react';
import { RotateCcw } from 'lucide-react';
import { getVersion } from '@tauri-apps/api/app';
import { Sidebar, PageType, ToastProvider, ConfirmDialogProvider } from './components';
import { isNewerVersion } from './utils/version';
import { DownloadProvider } from './components/DownloadContext';
import { DownloadBar } from './components/DownloadBar';
import { TitleBar } from './components/TitleBar';
import { SettingsDialog } from './components/SettingsDialog';

import { useI18n } from './hooks/useI18n';

// Zustand stores
import { useToolsStore } from './stores/toolsStore';
import { useNavigationStore } from './stores/navigationStore';

// Pages
import { ModelNexusProvider, ModelNexusTitleActions, ModelNexusMain, ModelNexusPanel, AddModelModal } from './pages/ModelNexus';

import { AppManagerProvider, AppManagerMain, AppManagerPanel, AppManagerBottom, AppManagerErrorModal } from './pages/AppManager';
import { LocalServerProvider, LocalServerMain, LocalServerPanel, LocalServerBottom } from './pages/LocalServer';
import { MotherAgentProvider, MotherAgentMain, MotherAgentPanel } from './pages/MotherAgent';
import { AiPulseProvider, AiPulseTitleActions, AiNewsMain, AiProjectsMain, AiPulsePanel } from './pages/AiPulse';
import { AiCoursesProvider, AiCoursesTitleActions, AiCoursesMain, AiCoursesPanel } from './pages/AiCourses';

function SidebarConnected({ onSettingsClick }: { onSettingsClick: () => void }) {
    // Selector form (one field per call) so unrelated store fields like
    // agentRunning ticking or motherNewMessage flipping don't re-render the
    // sidebar — and through it, the whole app tree on tab switches.
    const activePage        = useNavigationStore(s => s.activePage);
    const setActivePage     = useNavigationStore(s => s.setActivePage);
    const agentRunning      = useNavigationStore(s => s.agentRunning);
    const motherNewMessage  = useNavigationStore(s => s.motherNewMessage);
    const clearMotherBadge  = useNavigationStore(s => s.clearMotherBadge);
    const updateAvailable   = useNavigationStore(s => s.updateAvailable);
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

    // Stores — selector form so App.tsx (the root of the entire tree) only
    // re-renders when activePage flips, not on every motherNewMessage tick.
    const activePage         = useNavigationStore(s => s.activePage);
    const setUpdateAvailable = useNavigationStore(s => s.setUpdateAvailable);
    const scanTools = useToolsStore(s => s.scanTools);

    // ── Post-mount work — the window is already shown (main.tsx fires
    // appReady after first paint). Scan installed tools and check for app
    // updates in the background; both are non-blocking.
    useEffect(() => {
        const preload = async () => {
            try { await scanTools(); } catch { /* continue anyway */ }
            try {
                const [appVersion, res] = await Promise.all([
                    getVersion().catch(() => ''),
                    fetch('https://echobird.ai/api/version/index.json'),
                ]);
                if (res.ok && appVersion) {
                    const data = await res.json();
                    if (data.version && isNewerVersion(data.version, appVersion)) {
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
                              <AiPulseProvider>
                                <AiCoursesProvider>

                                    <AppManagerProvider>
                                        <LocalServerProvider>

                                            <div className="flex flex-col h-screen w-full bg-cyber-bg">
                                                {/* Title bar */}
                                                <TitleBar onSettingsClick={() => setShowSettings(true)} />
                                                <div className="flex flex-1 overflow-hidden text-cyber-text font-sans p-4 gap-0 relative isolate">
                                                    {/* Sidebar */}
                                                    <SidebarConnected onSettingsClick={() => setShowSettings(true)} />

                                                    {/* Main content wrapper — transparent against page bg, Claude-style */}
                                                    <div className="flex-1 flex flex-col overflow-hidden">

                                                        {/* Main + Right panel row */}
                                                        <div className="flex-1 flex gap-3 overflow-hidden">
                                                            <main className="flex-1 flex flex-col overflow-hidden">
                                                                <section className="flex-1 flex flex-col overflow-hidden pr-2">

                                                                    {/* Shared page title bar — fixed-height row so the title sits at the same baseline whether the page has tall action buttons or none */}
                                                                    <div className="mb-5 flex-shrink-0 flex items-center gap-3 h-9">
                                                                        <div className="flex items-baseline gap-3 flex-1 min-w-0">
                                                                            <h2 className="cjk-title flex-shrink-0">
                                                                                {is('news') && t('page.news')}
                                                                                {is('projects') && t('page.projects')}
                                                                                {is('courses') && t('page.courses')}
                                                                                {is('models') && t('page.modelNexus')}

                                                                                {is('apps') && t('page.appManager')}
                                                                                {is('localLlm') && t('page.localServer')}
                                                                                {is('mother') && t('page.motherAgent')}
                                                                            </h2>
                                                                            <div className="page-kicker truncate" aria-hidden="true">
                                                                                {is('news') && 'PULSE'}
                                                                                {is('projects') && 'RISING'}
                                                                                {is('courses') && 'ACADEMY'}
                                                                                {is('models') && 'ROSTER'}
                                                                                {is('apps') && 'STUDIO'}
                                                                                {is('localLlm') && 'RUNTIME'}
                                                                                {is('mother') && 'AGENT'}
                                                                            </div>
                                                                        </div>
                                                                        {/* Title actions — always mounted but hidden */}

                                                                        <span className={page(is('news') || is('projects'))}><AiPulseTitleActions /></span>
                                                                        <span className={page(is('courses'))}><AiCoursesTitleActions /></span>
                                                                        <span className={page(is('models'))}><ModelNexusTitleActions /></span>

                                                                        {is('mother') && (
                                                                            <div className="flex-shrink-0 flex items-center gap-2">
                                                                                <button
                                                                                    onClick={() => window.dispatchEvent(new CustomEvent('clear-chat'))}
                                                                                    className="p-1.5 rounded-lg text-cyber-text/40 hover:text-cyber-text hover:bg-cyber-text/10 transition-colors"
                                                                                >
                                                                                    <RotateCcw size={14} />
                                                                                </button>
                                                                            </div>
                                                                        )}
                                                                    </div>

                                                                    {/* Page content — always mounted, CSS hidden */}
                                                                    <div className={pageScroll(is('news'))}><AiNewsMain /></div>
                                                                    <div className={pageScroll(is('projects'))}><AiProjectsMain /></div>
                                                                    <div className={pageScroll(is('courses'))}><AiCoursesMain /></div>
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
                                                                    <div className={page(is('news') || is('projects'))}><AiPulsePanel /></div>
                                                                    <div className={page(is('courses'))}><AiCoursesPanel /></div>
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

                                </AiCoursesProvider>
                              </AiPulseProvider>
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
