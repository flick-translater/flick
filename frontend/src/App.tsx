import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import Sidebar from './components/Sidebar';
import TopBar from './components/TopBar';
import GeneralSettings from './views/GeneralSettings';
import HistoryView from './views/HistoryView';
import OCRSettings from './views/OCRSettings';
import AISettings from './views/AISettings';
import { ViewState } from './types';

export default function App() {
  const [currentView, setCurrentView] = useState<ViewState>('general');

  return (
    <div className="flex h-screen min-h-0 flex-col bg-surface font-sans text-on-surface lg:flex-row">
      <Sidebar currentView={currentView} setCurrentView={setCurrentView} />
      
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <TopBar 
          title={getViewTitle(currentView)} 
          onToggleWidget={() => {
            void invoke('show_translation_widget');
          }}
        />
        
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

function getViewTitle(view: ViewState) {
  switch (view) {
    case 'general': return 'General Settings';
    case 'history': return 'Capture History';
    case 'ocr': return 'OCR Settings';
    case 'ai': return 'AI Configuration';
  }
}
