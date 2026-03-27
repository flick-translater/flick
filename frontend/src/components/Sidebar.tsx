import React from 'react';
import { Settings, History, ScanText, Bot } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { ViewState } from '../types';

interface SidebarProps {
  currentView: ViewState;
  setCurrentView: (view: ViewState) => void;
}

export default function Sidebar({ currentView, setCurrentView }: SidebarProps) {
  const { t } = useTranslation();

  const navItems: { id: ViewState; label: string; icon: React.ElementType }[] = [
    { id: 'general', label: t('sidebar.general'), icon: Settings },
    { id: 'history', label: t('sidebar.history'), icon: History },
    { id: 'ocr', label: t('sidebar.ocr'), icon: ScanText },
    { id: 'ai', label: t('sidebar.ai'), icon: Bot },
  ];

  return (
    <aside className="z-10 shrink-0 border-b border-outline-variant/20 bg-surface-container-low lg:w-64 lg:border-b-0 lg:border-r">
      <div className="flex items-center gap-3 px-4 py-4 sm:px-6 lg:mb-6 lg:py-6">
        <div>
          <h1 className="font-headline text-2xl font-black leading-tight tracking-tight text-primary sm:text-3xl">Flick</h1>
        </div>
      </div>

      <nav className="flex gap-2 overflow-x-auto px-3 pb-4 lg:flex-1 lg:flex-col lg:space-y-1 lg:overflow-visible lg:px-3 lg:pb-6">
        {navItems.map((item) => {
          const isActive = currentView === item.id;
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              onClick={() => setCurrentView(item.id)}
              className={`flex shrink-0 items-center gap-3 rounded-xl px-3 py-2.5 text-xs font-bold uppercase tracking-wide transition-all duration-200 sm:px-4 sm:text-sm lg:w-full ${
                isActive 
                  ? 'bg-surface-container-lowest text-primary shadow-sm scale-100' 
                  : 'text-on-surface-variant opacity-70 hover:opacity-100 hover:bg-surface-container hover:translate-x-1'
              }`}
            >
              <Icon size={20} className={isActive ? 'text-primary' : ''} />
              {item.label}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
