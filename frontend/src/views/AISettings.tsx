import { useState } from 'react';
import { Network, Key, Terminal, SlidersHorizontal, Eye, EyeOff, ChevronDown, Check } from 'lucide-react';
import { useTranslation } from 'react-i18next';

export default function AISettings() {
  const { t } = useTranslation();
  const [activeProvider, setActiveProvider] = useState('OpenAI');
  const [usingProvider, setUsingProvider] = useState(t('ai.providerOpenAI'));
  const [showKey, setShowKey] = useState(false);
  const [temperature, setTemperature] = useState(0.7);
  const [maxTokens, setMaxTokens] = useState(4096);

  const providers = [
    t('ai.providerOpenAI'),
    t('ai.providerAnthropic'),
    t('ai.providerOpenAICompatible'),
    t('ai.providerAnthropicCompatible'),
    t('ai.providerOllama'),
    t('ai.providerLMStudio'),
  ];

  return (
    <div className="mx-auto max-w-5xl animate-in fade-in duration-500">
      <section className="mb-6">
        <div className="flex items-center gap-2 mb-3">
          <Network className="text-primary" size={18} />
          <h3 className="text-xs font-bold uppercase tracking-widest text-on-surface-variant">{t('ai.providerSelection')}</h3>
        </div>
        <div className="flex flex-wrap gap-1 rounded-xl bg-surface-container-low p-1">
          {providers.map(p => (
            <button
              key={p}
              onClick={() => setActiveProvider(p)}
              className={`rounded-lg px-3 py-2 text-sm font-semibold transition-all flex items-center gap-1.5 ${
                activeProvider === p
                  ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-outline-variant/20'
                  : 'text-on-surface-variant hover:text-on-surface hover:bg-surface-container-lowest/50'
              }`}
            >
              <span className={`w-2 h-2 rounded-full ${usingProvider === p ? 'bg-primary' : 'bg-outline-variant'}`}></span>
              {p}
            </button>
          ))}
        </div>
      </section>

      <div className="mb-6 grid grid-cols-1 gap-4 lg:grid-cols-2 lg:gap-6">
        <section className="space-y-4">
          <div className="flex items-center gap-2 mb-1">
            <Key className="text-primary" size={18} />
            <h3 className="text-xs font-bold uppercase tracking-widest text-on-surface-variant">{t('ai.apiConfig')}</h3>
          </div>
          <div className="space-y-3">
            <div className="group">
              <label className="block text-xs font-bold text-on-surface-variant mb-1 ml-1">{t('ai.modelSelection')}</label>
              {activeProvider === t('ai.providerOpenAI') || activeProvider === t('ai.providerAnthropic') ? (
                <div className="relative">
                  <select className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all appearance-none cursor-pointer">
                    <option>{t('ai.modelGPT4Turbo')}</option>
                    <option>{t('ai.modelGPT4o')}</option>
                    <option>{t('ai.modelGPT35Turbo')}</option>
                  </select>
                  <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 text-on-surface-variant pointer-events-none" size={16} />
                </div>
              ) : (
                <input
                  type="text"
                  placeholder={t('ai.modelNamePlaceholder')}
                  className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all"
                />
              )}
            </div>
            <div className="group">
              <label className="block text-xs font-bold text-on-surface-variant mb-1 ml-1">{t('ai.apiAddress')}</label>
              <input
                type="text"
                placeholder={t('ai.apiAddressPlaceholder')}
                className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all"
              />
            </div>
            <div className="group">
              <label className="block text-xs font-bold text-on-surface-variant mb-1 ml-1">{t('ai.apiKey')}</label>
              <div className="flex gap-2">
                <div className="relative flex-1">
                  <input
                    type={showKey ? "text" : "password"}
                    placeholder={t('ai.apiKeyPlaceholder')}
                    className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all pr-10"
                  />
                  <button
                    onClick={() => setShowKey(!showKey)}
                    className="absolute right-3 top-1/2 -translate-y-1/2 text-on-surface-variant hover:text-primary transition-colors"
                  >
                    {showKey ? <EyeOff size={16} /> : <Eye size={16} />}
                  </button>
                </div>
                <button
                  onClick={() => {}}
                  className="px-3 py-2.5 bg-primary text-on-primary rounded-lg text-sm font-semibold hover:bg-primary/90 transition-all shadow-sm whitespace-nowrap"
                >
                  {t('ai.testConnection')}
                </button>
              </div>
            </div>
          </div>
        </section>

        <section className="flex flex-col h-full">
          <div className="flex items-center gap-2 mb-1">
            <Terminal className="text-primary" size={18} />
            <h3 className="text-xs font-bold uppercase tracking-widest text-on-surface-variant">{t('ai.defaultPrompt')}</h3>
          </div>
          <div className="flex-1 min-h-[140px]">
            <textarea
              className="w-full h-full p-3 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm resize-none leading-relaxed"
              placeholder={t('ai.defaultPromptPlaceholder')}
            ></textarea>
          </div>
        </section>
      </div>

      <section className="rounded-xl border border-outline-variant/20 bg-surface-container-low/50 p-4 shadow-sm">
        <div className="mb-4 flex items-center gap-2">
          <SlidersHorizontal className="text-primary" size={18} />
          <h3 className="text-xs font-bold uppercase tracking-widest text-on-surface-variant">{t('ai.advancedParameters')}</h3>
        </div>
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-2 lg:gap-8">
          <div className="space-y-2">
            <div className="flex justify-between items-center">
              <label className="text-xs font-bold text-on-surface-variant uppercase tracking-wider">{t('ai.maxTokens')}</label>
              <span className="px-2 py-0.5 bg-primary/10 text-primary text-xs font-black rounded-full">{maxTokens}</span>
            </div>
            <input
              type="number"
              value={maxTokens}
              onChange={(e) => setMaxTokens(Number(e.target.value))}
              className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all"
            />
            <p className="text-[10px] text-on-surface-variant font-medium">{t('ai.maxTokensDesc')}</p>
          </div>
          <div className="space-y-2">
            <div className="flex justify-between items-center">
              <label className="text-xs font-bold text-on-surface-variant uppercase tracking-wider">{t('ai.temperature')}</label>
              <span className="px-2 py-0.5 bg-primary/10 text-primary text-xs font-black rounded-full">{temperature.toFixed(1)}</span>
            </div>
            <div className="relative py-2">
              <input
                type="range"
                min="0"
                max="1"
                step="0.1"
                value={temperature}
                onChange={(e) => setTemperature(parseFloat(e.target.value))}
                className="w-full h-2 bg-surface-container-highest rounded-full appearance-none cursor-pointer accent-primary"
              />
              <div className="mt-2 flex justify-between gap-3 px-1">
                <span className="text-[10px] font-bold text-on-surface-variant">{t('ai.temperaturePrecise')}</span>
                <span className="text-right text-[10px] font-bold text-on-surface-variant">{t('ai.temperatureCreative')}</span>
              </div>
            </div>
          </div>
        </div>
      </section>

      <div className="flex justify-end gap-2 pt-4">
        <button
          onClick={() => {}}
          className="px-4 py-2 bg-surface-container-lowest border border-outline-variant/30 text-on-surface-variant rounded-lg text-sm font-semibold hover:bg-surface-container-low transition-all shadow-sm"
        >
          {t('ai.discard')}
        </button>
        <button
          onClick={() => setUsingProvider(activeProvider)}
          className="px-4 py-2 bg-primary text-on-primary rounded-lg text-sm font-semibold hover:bg-primary/90 transition-all shadow-sm flex items-center gap-1.5"
        >
          <Check size={14} />
          {t('ai.useProvider')}
        </button>
      </div>
    </div>
  );
}