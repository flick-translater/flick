import { Zap } from 'lucide-react';

interface TopBarProps {
  title: string;
  onToggleWidget: () => void;
}

export default function TopBar({ title, onToggleWidget }: TopBarProps) {
  return (
    <header className="sticky top-0 z-30 flex shrink-0 items-center justify-between gap-4 border-b border-outline-variant/10 bg-surface/80 px-4 py-3 backdrop-blur-md sm:px-6 lg:px-8">
      <div className="min-w-0 flex-1">
        <h2 className="truncate font-headline text-lg font-extrabold tracking-tight text-primary sm:text-xl lg:text-2xl">{title}</h2>
      </div>
      
      <button 
        onClick={onToggleWidget}
        className="flex shrink-0 items-center gap-2 rounded-lg p-2 text-primary transition-colors hover:bg-primary/10"
        title="Toggle Translation Widget"
      >
        <Zap size={20} className="fill-current" />
        <span className="hidden text-xs font-bold uppercase tracking-wider sm:inline-block">Widget</span>
      </button>
    </header>
  );
}
