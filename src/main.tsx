import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './index.css';
import { I18nProvider } from './hooks/useI18n';
import { useThemeStore } from './stores/themeStore';

// Resolve and apply the theme before first paint to avoid a flash.
useThemeStore.getState().init();

ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
        <I18nProvider>
            <App />
        </I18nProvider>
    </React.StrictMode>
);

