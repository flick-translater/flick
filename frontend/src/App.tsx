import { useState } from 'react';
import Sidebar from './components/Sidebar';
import TopBar from './components/TopBar';
import GeneralSettings from './views/GeneralSettings';
import HistoryView from './views/HistoryView';
import OCRSettings from './views/OCRSettings';
import AISettings from './views/AISettings';
import TranslationWidget from './components/TranslationWidget';
import { ViewState } from './types';

export default function App() {
  const [currentView, setCurrentView] = useState<ViewState>('general');
  const [showWidget, setShowWidget] = useState(false);

  return (
    <div className="flex h-screen bg-surface font-sans text-on-surface overflow-hidden">
      <Sidebar currentView={currentView} setCurrentView={setCurrentView} />
      
      <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
        <TopBar 
          title={getViewTitle(currentView)} 
          onToggleWidget={() => setShowWidget(!showWidget)} 
        />
        
        <main className="flex-1 overflow-y-auto p-8 lg:p-12 relative">
          {currentView === 'general' && <GeneralSettings />}
          {currentView === 'history' && <HistoryView />}
          {currentView === 'ocr' && <OCRSettings />}
          {currentView === 'ai' && <AISettings />}
        </main>
      </div>

      {showWidget && <TranslationWidget onClose={() => setShowWidget(false)} />}
    </div>
  );
}

function getViewTitle(view: ViewState) {
  switch (view) {
    case 'general': return 'General Settings';
    case 'history': return 'Capture History';
    case 'ocr': return 'OCR Settings';
    case 'ai': return 'AI Configuration';
  }
}
