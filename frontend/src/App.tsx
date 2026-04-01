import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import Sidebar from './components/Sidebar';
import TopBar from './components/TopBar';
import GeneralSettings from './views/GeneralSettings';
import HistoryView from './views/HistoryView';
import OCRSettings from './views/OCRSettings';
import AISettings from './views/AISettings';
import { ViewState } from './types';

export default function App() {
  const { t } = useTranslation();
  const [currentView, setCurrentView] = useState<ViewState>('general');

  return (
    <div className="flex h-screen min-h-0 flex-col bg-surface font-sans text-on-surface lg:flex-row">
      <Sidebar currentView={currentView} setCurrentView={setCurrentView} />
      
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <TopBar title={getViewTitle(currentView, t)} />
        
        <main className="relative flex-1 overflow-y-auto px-4 py-4 sm:px-6 sm:py-6 lg:px-8 lg:py-8 xl:px-10 xl:py-10">
          {currentView === 'general' && <GeneralSettings />}
          {currentView === 'history' && <HistoryView />}
          {currentView === 'ocr' && <OCRSettings />}
          {currentView === 'ai' && <AISettings />}
        </main>
      </div>
    </div>
  );
}

function getViewTitle(view: ViewState, t: (key: string) => string) {
  switch (view) {
    case 'general': return t('sidebar.general');
    case 'history': return t('sidebar.history');
    case 'ocr': return t('sidebar.ocr');
    case 'ai': return t('sidebar.ai');
  }
}
