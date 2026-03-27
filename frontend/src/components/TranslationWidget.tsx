import { useState } from 'react';
import { Pin, Minus, X, Copy, ArrowRightLeft, Volume2, Share2, History, Star, ScanText } from 'lucide-react';
import { useTranslation } from 'react-i18next';

interface TranslationWidgetProps {
  onClose: () => void;
}

export default function TranslationWidget({ onClose }: TranslationWidgetProps) {
  const { t } = useTranslation();
  const [isPinned, setIsPinned] = useState(false);

  return (
    <div className="fixed inset-x-3 bottom-3 top-24 z-50 flex max-h-[calc(100vh-7rem)] flex-col overflow-hidden rounded-xl border border-outline-variant/30 shadow-2xl duration-300 animate-in fade-in glass-panel sm:left-auto sm:right-4 sm:w-[min(480px,calc(100vw-2rem))] lg:right-8 lg:top-20 lg:h-[min(640px,calc(100vh-6rem))] lg:max-h-none">
      {/* Header */}
      <header className="flex justify-between items-center px-4 py-3 bg-white/80 border-b border-outline-variant/20 shrink-0">
        <div className="flex items-center gap-2">
          <img 
            src="https://lh3.googleusercontent.com/aida-public/AB6AXuBA7uviJf2q0QkZM9cIPRQTKrK48R2cd2xeSwM8K3ynoq89JoLeWTy5MDuIS3fuzZwdz61GftmVSQcsiLBKlJkQSqhN84xOrC4ort4exBYS9jB6lZEH4XEopxSK4i8Ymf8ESne7fMknWg4QmPVZrvNSvvtCSZtn1QBynRu5yIdbZmx5AdU0mqCOrN255nhN-FNqILXlmLLAHl2IyPS3a3fivdHzp4REfThQMjsWd5JPSinBMRSmrm7jDD1gr_jDce2E4ROHFLr2bE8" 
            alt="Flick Logo" 
            className="w-5 h-5 object-cover rounded"
          />
          <span className="font-headline font-bold text-sm tracking-tight text-on-surface">Flick</span>
        </div>
        <div className="flex gap-1">
          <button 
            onClick={() => setIsPinned(!isPinned)}
            className={`w-8 h-8 flex items-center justify-center rounded transition-colors ${isPinned ? 'bg-primary text-white' : 'text-on-surface-variant hover:bg-surface-container'}`}
          >
            <Pin size={16} className={isPinned ? 'fill-current' : ''} />
          </button>
          <button className="w-8 h-8 flex items-center justify-center rounded hover:bg-surface-container transition-colors text-on-surface-variant">
            <Minus size={16} />
          </button>
          <button onClick={onClose} className="w-8 h-8 flex items-center justify-center rounded hover:bg-error/10 hover:text-error transition-colors text-on-surface-variant">
            <X size={16} />
          </button>
        </div>
      </header>

      {/* Main Content */}
      <main className="flex flex-1 flex-col gap-4 overflow-hidden bg-surface/50 p-4 sm:p-5">
        {/* Source Text */}
        <section className="flex-1 flex flex-col min-h-0 bg-white border border-outline-variant/30 rounded-xl p-4 shadow-sm">
          <div className="flex items-center justify-between mb-3 border-b border-outline-variant/20 pb-2">
            <span className="text-[10px] uppercase font-bold tracking-[0.1em] text-outline">{t('widget.sourceText')}</span>
            <button className="text-on-surface-variant hover:text-primary transition-colors">
              <Copy size={16} />
            </button>
          </div>
          <div className="flex-1 overflow-y-auto custom-scrollbar pr-2">
            <p className="font-body text-sm leading-relaxed text-on-surface">
              The Digital Atrium design philosophy emphasizes spaciousness and clarity. By utilizing tonal layering instead of harsh borders, we create a desktop environment that feels like an extension of the user's focus, rather than a distraction. This screenshot tool captures the essence of efficient utility.
            </p>
          </div>
        </section>

        {/* Language Selector */}
        <div className="grid shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-2">
          <div className="relative min-w-0">
            <select className="w-full appearance-none py-2.5 px-3 rounded-lg border border-outline-variant/30 bg-white text-on-surface font-medium text-xs hover:border-primary/50 transition-all outline-none cursor-pointer shadow-sm">
              <option>{t('widget.english')}</option>
            </select>
            <div className="absolute inset-y-0 right-3 flex items-center pointer-events-none text-on-surface-variant">
              <svg width="10" height="6" viewBox="0 0 10 6" fill="none" xmlns="http://www.w3.org/2000/svg">
                <path d="M1.175 0.150024L5 3.97502L8.825 0.150024L10 1.32502L5 6.32502L0 1.32502L1.175 0.150024Z" fill="currentColor"/>
              </svg>
            </div>
          </div>
          
          <button className="w-10 h-10 rounded-lg bg-primary text-white flex items-center justify-center shadow-md hover:bg-primary-container transition-all active:scale-95 shrink-0">
            <ArrowRightLeft size={18} />
          </button>
          
          <div className="relative min-w-0">
            <select className="w-full appearance-none py-2.5 px-3 rounded-lg border border-outline-variant/30 bg-white text-on-surface font-medium text-xs hover:border-primary/50 transition-all outline-none cursor-pointer shadow-sm">
              <option>{t('widget.chinese')}</option>
            </select>
            <div className="absolute inset-y-0 right-3 flex items-center pointer-events-none text-on-surface-variant">
              <svg width="10" height="6" viewBox="0 0 10 6" fill="none" xmlns="http://www.w3.org/2000/svg">
                <path d="M1.175 0.150024L5 3.97502L8.825 0.150024L10 1.32502L5 6.32502L0 1.32502L1.175 0.150024Z" fill="currentColor"/>
              </svg>
            </div>
          </div>
        </div>

        {/* Translation Result */}
        <section className="flex-1 flex flex-col min-h-0 bg-primary-container/10 border border-primary/20 rounded-xl p-4 shadow-sm">
          <div className="flex items-center justify-between mb-3 border-b border-primary/10 pb-2">
            <span className="text-[10px] uppercase font-bold tracking-[0.1em] text-primary">{t('widget.translation')}</span>
            <div className="flex gap-3">
              <button className="text-primary hover:text-primary-container transition-colors">
                <Volume2 size={16} />
              </button>
              <button className="text-primary hover:text-primary-container transition-colors">
                <Share2 size={16} />
              </button>
            </div>
          </div>
          <div className="flex-1 overflow-y-auto custom-scrollbar pr-2">
            <p className="font-body text-sm leading-relaxed text-primary-container font-medium">
              Digital Atrium 设计哲学强调空间感和清晰度。通过利用色调分层而非生硬的边框，我们创造了一个桌面环境，让用户感觉它像是专注力的延伸，而不是干扰。这款截屏工具捕捉了高效实用的精髓。
            </p>
          </div>
        </section>
      </main>

      {/* Footer */}
      <footer className="px-5 py-4 bg-white/90 border-t border-outline-variant/20 flex justify-end items-center shrink-0">
        <button className="bg-primary text-white px-6 py-2.5 rounded-lg text-sm font-bold flex items-center gap-2 shadow-md hover:bg-primary-container transition-all active:scale-95">
          <ScanText size={16} />
          <span>{t('widget.translate')}</span>
        </button>
      </footer>
    </div>
  );
}
