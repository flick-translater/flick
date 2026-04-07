import { useState, useEffect } from 'react';
import { Network, Key, Terminal, SlidersHorizontal, Eye, EyeOff, ChevronDown, Check } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import type { AISettings, AiTestResult, AppSettings, ProviderSettings } from '../types';

const defaultProviderSettings: ProviderSettings = {
  api_key: '',
  api_base_url: 'https://api.openai.com/v1',
  model: 'gpt-4o-mini',
  temperature: 0.7,
  max_tokens: 4096,
  default_prompt: '',
};

const defaultBaseUrlMap: Record<string, string> = {
  openai: 'https://api.openai.com/v1',
  anthropic: 'https://api.anthropic.com/v1',
  openai_compatible: 'https://api.openai.com/v1',
  anthropic_compatible: 'https://api.anthropic.com/v1',
  ollama: 'http://localhost:11434/v1',
  lmstudio: 'http://localhost:1234/v1',
};

export default function AISettings() {
  const { t } = useTranslation();
  const [isLoading, setIsLoading] = useState(true);
  const [aiSettings, setAiSettings] = useState<AISettings>({
    active_provider: '',
    openai: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.openai },
    anthropic: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.anthropic },
    openai_compatible: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.openai_compatible },
    anthropic_compatible: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.anthropic_compatible },
    ollama: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.ollama },
    lmstudio: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.lmstudio },
  });
  const [savedSettings, setSavedSettings] = useState<AISettings | null>(null);
  const [selectedProvider, setSelectedProvider] = useState('');
  const [showKey, setShowKey] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<AiTestResult | null>(null);

  useEffect(() => {
    loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      const appSettings = await invoke<AppSettings>('get_app_settings');
      const ai = appSettings.ai || {
        active_provider: '',
        openai: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.openai },
        anthropic: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.anthropic },
        openai_compatible: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.openai_compatible },
        anthropic_compatible: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.anthropic_compatible },
        ollama: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.ollama },
        lmstudio: { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap.lmstudio },
      };
      
      // Ensure all provider settings exist with defaults
      const ensureProviderSettings = (settings: ProviderSettings | undefined): ProviderSettings => ({
        ...defaultProviderSettings,
        ...(settings || {}),
      });
      
      const normalizedAi: AISettings = {
        active_provider: ai.active_provider || '',
        openai: ensureProviderSettings(ai.openai),
        anthropic: ensureProviderSettings(ai.anthropic),
        openai_compatible: ensureProviderSettings(ai.openai_compatible),
        anthropic_compatible: ensureProviderSettings(ai.anthropic_compatible),
        ollama: ensureProviderSettings(ai.ollama),
        lmstudio: ensureProviderSettings(ai.lmstudio),
      };
      
      setAiSettings(normalizedAi);
      setSavedSettings(normalizedAi);
      setSelectedProvider(normalizedAi.active_provider);
    } catch (error) {
      console.error('Failed to load settings:', error);
    } finally {
      setIsLoading(false);
    }
  };

  const getCurrentSettings = (): ProviderSettings => {
    const provider = selectedProvider || 'openai';
    const key = provider as keyof AISettings;
    if (key === 'active_provider') {
      return defaultProviderSettings;
    }
    const settings = aiSettings[key];
    if (!settings) {
      return { ...defaultProviderSettings, api_base_url: defaultBaseUrlMap[provider] || defaultBaseUrlMap.openai };
    }
    return settings as ProviderSettings;
  };

  const updateCurrentSettings = (updates: Partial<ProviderSettings>) => {
    const key = selectedProvider as keyof AISettings;
    if (key === 'active_provider') return;
    setAiSettings(prev => {
      const prevSettings = prev[key] as ProviderSettings | undefined;
      return {
        ...prev,
        [key]: {
          ...(prevSettings || defaultProviderSettings),
          ...updates,
        },
      };
    });
  };

  const handleSave = async (applyProvider = false) => {
    setIsSaving(true);
    try {
      const nextSettings = applyProvider
        ? { ...aiSettings, active_provider: selectedProvider }
        : aiSettings;
      await invoke<AppSettings>('update_ai_settings', { aiSettings: nextSettings });
      setAiSettings(nextSettings);
      setSavedSettings(nextSettings);
    } catch (error) {
      console.error('Failed to save settings:', error);
    } finally {
      setIsSaving(false);
    }
  };

  const handleTestConnection = async () => {
    if (!selectedProvider) {
      return;
    }
    setIsTesting(true);
    setTestResult(null);
    try {
      const testingSettings = { ...aiSettings, active_provider: selectedProvider };
      const result = await invoke<AiTestResult>('test_ai_connection', { aiSettings: testingSettings });
      setTestResult(result);
    } catch (error) {
      console.error('Connection test failed:', error);
      setTestResult({
        ok: false,
        provider: selectedProvider,
        protocol: 'unknown',
        model: currentSettings.model,
        latency_ms: 0,
        message: String(error),
      });
    } finally {
      setIsTesting(false);
    }
  };

  const handleDiscard = () => {
    if (savedSettings) {
      setAiSettings(savedSettings);
      setSelectedProvider(savedSettings.active_provider);
      setTestResult(null);
    }
  };

  const handleProviderChange = (provider: string) => {
    if (!provider) {
      setSelectedProvider('');
      setTestResult(null);
      return;
    }
    setAiSettings(prev => {
      const key = provider as keyof AISettings;
      if (key === 'active_provider') return prev;
      const existingSettings = (prev[key] as ProviderSettings | undefined) || defaultProviderSettings;
      const baseUrl = defaultBaseUrlMap[provider] || defaultBaseUrlMap.openai;
      return {
        ...prev,
        [key]: {
          ...existingSettings,
          api_base_url: existingSettings.api_base_url || baseUrl,
        },
      };
    });
    setSelectedProvider(provider);
    setTestResult(null);
  };

  const hasConfigChanges = savedSettings != null && JSON.stringify(aiSettings) !== JSON.stringify(savedSettings);
  const canApplyProvider = savedSettings != null && selectedProvider !== savedSettings.active_provider;
  const currentSettings = getCurrentSettings();
  const hasSelectedProvider = selectedProvider.length > 0;

  const providers = [
    { key: 'openai', label: t('ai.providerOpenAI') },
    { key: 'anthropic', label: t('ai.providerAnthropic') },
    { key: 'openai_compatible', label: t('ai.providerOpenAICompatible') },
    { key: 'anthropic_compatible', label: t('ai.providerAnthropicCompatible') },
    { key: 'ollama', label: t('ai.providerOllama') },
    { key: 'lmstudio', label: t('ai.providerLMStudio') },
  ];

  const openaiModels = [
    { key: 'gpt-4-turbo-preview', label: t('ai.modelGPT4Turbo') },
    { key: 'gpt-4o', label: t('ai.modelGPT4o') },
    { key: 'gpt-3.5-turbo', label: t('ai.modelGPT35Turbo') },
  ];

  const isStandardProvider = selectedProvider === 'openai' || selectedProvider === 'anthropic';

  if (isLoading) {
    return (
      <div className="mx-auto max-w-5xl animate-in fade-in duration-500">
        <div className="flex items-center justify-center h-64">
          <div className="text-on-surface-variant">Loading...</div>
        </div>
      </div>
    );
  }

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
              key={p.key}
              onClick={() => handleProviderChange(p.key)}
              className={`rounded-lg px-3 py-2 text-sm font-semibold transition-all flex items-center gap-1.5 ${
                selectedProvider === p.key
                  ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-outline-variant/20'
                  : 'text-on-surface-variant hover:text-on-surface hover:bg-surface-container-lowest/50'
              }`}
            >
              <span className={`w-2 h-2 rounded-full ${savedSettings?.active_provider === p.key ? 'bg-primary' : 'bg-outline-variant'}`}></span>
              {p.label}
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
              {isStandardProvider ? (
                <div className="relative">
                  <select 
                    value={currentSettings.model}
                    onChange={(e) => updateCurrentSettings({ model: e.target.value })}
                    disabled={!hasSelectedProvider}
                    className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all appearance-none cursor-pointer"
                  >
                    {openaiModels.map(m => (
                      <option key={m.key} value={m.key}>{m.label}</option>
                    ))}
                  </select>
                  <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 text-on-surface-variant pointer-events-none" size={16} />
                </div>
              ) : (
                <input
                  type="text"
                  value={currentSettings.model}
                  onChange={(e) => updateCurrentSettings({ model: e.target.value })}
                  placeholder={t('ai.modelNamePlaceholder')}
                  disabled={!hasSelectedProvider}
                  className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all"
                />
              )}
            </div>
            <div className="group">
              <label className="block text-xs font-bold text-on-surface-variant mb-1 ml-1">{t('ai.apiAddress')}</label>
              <input
                type="text"
                value={currentSettings.api_base_url}
                onChange={(e) => updateCurrentSettings({ api_base_url: e.target.value })}
                placeholder={t('ai.apiAddressPlaceholder')}
                disabled={!hasSelectedProvider}
                className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all"
              />
            </div>
            <div className="group">
              <label className="block text-xs font-bold text-on-surface-variant mb-1 ml-1">{t('ai.apiKey')}</label>
              <div className="flex gap-2">
                <div className="relative flex-1">
                  <input
                    type={showKey ? "text" : "password"}
                    value={currentSettings.api_key}
                    onChange={(e) => updateCurrentSettings({ api_key: e.target.value })}
                    placeholder={t('ai.apiKeyPlaceholder')}
                    disabled={!hasSelectedProvider}
                    className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all pr-10"
                  />
                  <button
                    onClick={() => setShowKey(!showKey)}
                    disabled={!hasSelectedProvider}
                    className="absolute right-3 top-1/2 -translate-y-1/2 text-on-surface-variant hover:text-primary transition-colors"
                  >
                    {showKey ? <EyeOff size={16} /> : <Eye size={16} />}
                  </button>
                </div>
                <button
                  onClick={handleTestConnection}
                  disabled={isTesting || !hasSelectedProvider || !currentSettings.api_key}
                  className="px-3 py-2.5 bg-primary text-on-primary rounded-lg text-sm font-semibold hover:bg-primary/90 transition-all shadow-sm whitespace-nowrap disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {isTesting ? '...' : t('ai.testConnection')}
                </button>
              </div>
              {testResult && (
                <div
                  className={`mt-2 rounded-lg border px-3 py-2 text-xs ${
                    testResult.ok
                      ? 'border-green-500/20 bg-green-500/10 text-green-700'
                      : 'border-red-500/20 bg-red-500/10 text-red-700'
                  }`}
                >
                  <div className="font-semibold">{testResult.message}</div>
                  <div className="mt-1 opacity-80">
                    {`${testResult.provider} / ${testResult.protocol} / ${testResult.model}`}
                    {testResult.latency_ms > 0 ? ` / ${testResult.latency_ms}ms` : ''}
                  </div>
                </div>
              )}
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
              value={currentSettings.default_prompt}
              onChange={(e) => updateCurrentSettings({ default_prompt: e.target.value })}
              disabled={!hasSelectedProvider}
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
              <span className="px-2 py-0.5 bg-primary/10 text-primary text-xs font-black rounded-full">{currentSettings.max_tokens}</span>
            </div>
            <input
              type="number"
              value={currentSettings.max_tokens}
              onChange={(e) => updateCurrentSettings({ max_tokens: Number(e.target.value) })}
              disabled={!hasSelectedProvider}
              className="w-full px-3 py-2.5 bg-surface-container-lowest border border-outline-variant/20 rounded-lg text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all"
            />
            <p className="text-[10px] text-on-surface-variant font-medium">{t('ai.maxTokensDesc')}</p>
          </div>
          <div className="space-y-2">
            <div className="flex justify-between items-center">
              <label className="text-xs font-bold text-on-surface-variant uppercase tracking-wider">{t('ai.temperature')}</label>
              <span className="px-2 py-0.5 bg-primary/10 text-primary text-xs font-black rounded-full">{currentSettings.temperature.toFixed(1)}</span>
            </div>
            <div className="relative py-2">
              <input
                type="range"
                min="0"
                max="1"
                step="0.1"
                value={currentSettings.temperature}
                onChange={(e) => updateCurrentSettings({ temperature: parseFloat(e.target.value) })}
                disabled={!hasSelectedProvider}
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
          onClick={handleDiscard}
          disabled={!hasConfigChanges && !canApplyProvider}
          className="px-4 py-2 bg-surface-container-lowest border border-outline-variant/30 text-on-surface-variant rounded-lg text-sm font-semibold hover:bg-surface-container-low transition-all shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {t('ai.discard')}
        </button>
        <button
          onClick={() => handleSave(false)}
          disabled={!hasConfigChanges || isSaving}
          className="px-4 py-2 bg-primary text-on-primary rounded-lg text-sm font-semibold hover:bg-primary/90 transition-all shadow-sm flex items-center gap-1.5 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Check size={14} />
          {isSaving ? '...' : t('ai.saveConfig')}
        </button>
        <button
          onClick={() => handleSave(true)}
          disabled={(!hasConfigChanges && !canApplyProvider) || isSaving}
          className="px-4 py-2 bg-secondary text-on-secondary rounded-lg text-sm font-semibold hover:bg-secondary/90 transition-all shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {isSaving ? '...' : t('ai.useProvider')}
        </button>
      </div>
    </div>
  );
}
