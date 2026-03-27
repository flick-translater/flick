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
    <aside className="w-64 bg-surface-container-low border-r border-outline-variant/20 flex flex-col py-6 shrink-0 z-10">
      <div className="px-6 mb-10 flex items-center gap-3">
        <div>
          <h1 className="text-3xl font-black text-primary font-headline leading-tight tracking-tight">Flick</h1>
        </div>
      </div>

      <nav className="flex-1 px-3 space-y-1">
        {navItems.map((item) => {
          const isActive = currentView === item.id;
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              onClick={() => setCurrentView(item.id)}
              className={`w-full flex items-center gap-3 px-4 py-3 rounded-xl text-sm font-bold tracking-wide uppercase transition-all duration-200 ${
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
