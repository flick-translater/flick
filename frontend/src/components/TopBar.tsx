import { Zap } from 'lucide-react';

interface TopBarProps {
  title: string;
  onToggleWidget: () => void;
}

export default function TopBar({ title, onToggleWidget }: TopBarProps) {
  return (
    <header className="h-20 px-8 flex items-center justify-between bg-surface/80 backdrop-blur-md sticky top-0 z-30 shrink-0">
      <div className="flex items-center gap-8">
        <h2 className="text-2xl font-extrabold text-primary font-headline tracking-tight hidden lg:block">{title}</h2>
      </div>
      
      <div className="flex items-center gap-6">
        <div className="flex items-center gap-2">
          <button 
            onClick={onToggleWidget}
            className="p-2 text-primary hover:bg-primary/10 rounded-lg transition-colors flex items-center gap-2 mr-2"
            title="Toggle Translation Widget"
          >
            <Zap size={20} className="fill-current" />
            <span className="text-xs font-bold uppercase tracking-wider hidden sm:inline-block">Widget</span>
          </button>
        </div>
      </div>
    </header>
  );
}
