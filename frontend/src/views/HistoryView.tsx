import { useState } from 'react';
import { ImageIcon, Code, Brush, LineChart, Video, Globe, Trash2, Copy, Clock } from 'lucide-react';
import { useTranslation } from 'react-i18next';

const screenshots = [
  { id: 1, name: 'Analytics_Dashboard.png', date: 'Oct 24, 2023', size: '1.2 MB', icon: LineChart, img: 'https://lh3.googleusercontent.com/aida-public/AB6AXuDgScOBozTJmnvXLQh-TYyyaePv9D5THwI2d7smpRdlpKxP1CUlHbYaB4PnwFSvwAWLdj4hGtyYDci-dxSAZtiEtH3dfo3CLC4ChMx8zGMXyNuWFwmn3qYg9cobz-zq4Rg4mOIY0o3di1V1aLwlbPjf6z1zXFzwf009mH71xONAInLwZMzmAsTJRcprkrlv-n6fOgR8twhi4CjeYXgvofaW4X_4nLUFMomS1GkuJjxn2SfcWlOQDM2tuFNSUrhIdL9Gpn8LlVbD0DE' },
  { id: 2, name: 'Code_Snippet.png', date: 'Oct 23, 2023', size: '450 KB', icon: Code, img: 'https://lh3.googleusercontent.com/aida-public/AB6AXuAcB0ck2dvDLZrA3RdqODcuch3tsIbqo25QKtjScUWwFoyIiLOvL6aWvba2dloDww2mJ14bX-R5XNxS7R55AEEil_EzqjhrPGDRjyBzBSLbVKdxCOA8TqbNsAQDx5mZhGEcVIyL5WHN30omVwdsJlvuHGurxSWlzbb9ObkCtyOVK5c9nr5YianYl9UC8O6yRX62HcGzh8awRO3LMdIvmgpERe0-9WZxiPU7Z6d83FLmq2y7kB0wGdSyMj2YqF_W6gqUPWfIFP45oUw' },
  { id: 3, name: 'Design_Review_Notes.png', date: 'Oct 22, 2023', size: '2.4 MB', icon: Brush, img: 'https://lh3.googleusercontent.com/aida-public/AB6AXuCam-oe81Hic87FUUJ0gcd0zjhkuFMSWb3_flRm25TtwuzaJZQ38_I71LMCnQmG5t--MLT872DY0KaM75N_KGFUGhVd5dCCxGyUVYyQ7ZfudHpHvyDIyucKB5vB7RKYat1OJjBk4gbH6ncPf8TjjRI_3PJduFP7BuxPcay4XGYHHp56_aQkt8XfCaXn9NvAchuyIzFR-1GouoHp0sbUqr4xSfPtrCKEXup6Ewbd38dwx2exD6gVCzldJFLvVb_WvmaxFfvJuLo6Ols' },
  { id: 4, name: 'Marketing_Metrics.png', date: 'Oct 21, 2023', size: '890 KB', icon: LineChart, img: 'https://lh3.googleusercontent.com/aida-public/AB6AXuC_azywbqceSMXXegDWL7OUiI1X5ScwhEVSMRxvaPYLtsgitFKIIKPhVhwzUVuUnOAPtnXM8cWt7wfSUJOYtC_Nqw0jZO6Yu-d29I0Q4RlKiuUShKCyW1vpv7QzbXsAvATTtzEqpfBZabQ5h7l7j63Klc0aM_B4kbR7I5rPc1KI0WSpC72j_D1tyti7zyFVGRVRyMhsZ8qz4sg57rkpaR3AEDdw2OlL1JkdVpv2WniSNefDTb9QxWgfAqf1B_u9-TEOSUS6HPCxOJM' },
  { id: 5, name: 'Zoom_Meeting_Slide.png', date: 'Oct 20, 2023', size: '3.1 MB', icon: Video, img: 'https://lh3.googleusercontent.com/aida-public/AB6AXuDSeApbIYPyn8FZk7ZLnQzQr-d3qu9dF3cN2dUnSMX54Rb4H5ufzu9pe2RofuKtv9wXX4yOmQlHWLVX1qPw8iFld5SiEhm2ilGZV62f0NDSZQLjuxQ8lhVplk3h5dC1uHemuUtddF8apkmdWehQts34SnaOqR_cWcCdZmXOb8cEpriHEMKf3FWoGqctWCs7nDNb99db99pOI2g4WFEKs2IjMSSiA02wMhEb5FDQN78MYgfWMtUsG1ESeorQav3eHJszNQNikDtlLf8' },
  { id: 6, name: 'Web_Research_Snippet.png', date: 'Oct 19, 2023', size: '1.7 MB', icon: Globe, img: 'https://lh3.googleusercontent.com/aida-public/AB6AXuB_OO-DLZf0XHtzT3BtYBwoC2jT1_gyy2qwBPAaLEOy0tx_Qm0cvY96rOztLvKq-CxcO5VlN2T7Bbx3azFuvrNgOv-jv8Lx475FGYkd1_yyJbyq92jeSVCKvHs-UJ3O6vDmASB84GzFCnv6HLle5o1KG0UzcSq0EM7JK9WtjdBkj4fHEz1GNl-SATk-io7Ka6JnjK0UwRPdqf_JMy1IfiOHgrRiyckIoFEnqKmjVnh4WY8CFgfpaYqryB_zv60VnJZ1ROe-pMik-uw' },
];

const translations = [
  { id: 1, sourceLang: 'Japanese', targetLang: 'English', sourceText: '明日の会議の資料を、午後三時までに共有してください。', targetText: "Please share the materials for tomorrow's meeting by 3:00 PM.", time: 'Today, 10:45 AM' },
  { id: 2, sourceLang: 'German', targetLang: 'English', sourceText: 'Die Effizienz des Systems wurde durch die neuen AI-Modelle erheblich gesteigert.', targetText: 'The efficiency of the system has been significantly increased by the new AI models.', time: 'Yesterday, 4:12 PM' },
  { id: 3, sourceLang: 'Spanish', targetLang: 'English', sourceText: 'El diseño de la interfaz debe ser tanto funcional como estéticamente agradable.', targetText: 'The design of the interface must be both functional and aesthetically pleasing.', time: 'Oct 12, 09:20 AM', highlight: true },
];

export default function HistoryView() {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<'screenshots' | 'translations'>('screenshots');

  return (
    <div className="max-w-6xl mx-auto animate-in fade-in duration-500">
      <div className="flex items-center gap-2 bg-surface-container p-1.5 rounded-2xl w-fit mb-10 shadow-sm border border-outline-variant/20">
        <button 
          onClick={() => setActiveTab('screenshots')}
          className={`px-6 py-2.5 rounded-xl text-sm font-bold transition-all ${
            activeTab === 'screenshots' 
              ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-black/5' 
              : 'text-on-surface-variant hover:text-on-surface hover:bg-surface-container-high'
          }`}
        >
          {t('history.screenshotHistory')}
        </button>
        <button 
          onClick={() => setActiveTab('translations')}
          className={`px-6 py-2.5 rounded-xl text-sm font-bold transition-all ${
            activeTab === 'translations' 
              ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-black/5' 
              : 'text-on-surface-variant hover:text-on-surface hover:bg-surface-container-high'
          }`}
        >
          {t('history.translationHistory')}
        </button>
      </div>

      {activeTab === 'screenshots' ? (
        <>
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
            {screenshots.map((shot) => {
              const Icon = shot.icon;
              return (
                <div key={shot.id} className="group bg-surface-container-lowest rounded-xl shadow-sm ring-1 ring-outline-variant/30 overflow-hidden flex flex-col transition-all hover:shadow-xl hover:-translate-y-1">
                  <div className="aspect-video relative overflow-hidden bg-surface-container">
                    <img src={shot.img} alt={shot.name} className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-500" />
                    <div className="absolute inset-0 bg-primary/0 group-hover:bg-primary/5 transition-colors" />
                  </div>
                  <div className="p-5 flex flex-col gap-4">
                    <div className="flex items-start justify-between">
                      <div className="overflow-hidden pr-2">
                        <p className="text-xs text-on-surface-variant mt-1">{shot.date} • {shot.size}</p>
                      </div>
                    </div>
                    <div className="flex gap-2 pt-2 border-t border-outline-variant/20">
                      <button className="flex-1 bg-surface-container hover:bg-surface-container-high text-primary text-xs font-bold py-2.5 rounded-lg transition-colors">
                        {t('history.view')}
                      </button>
                      <button className="p-2.5 text-error hover:bg-error/10 rounded-lg transition-colors">
                        <Trash2 size={16} />
                      </button>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
          
          <div className="flex items-center justify-between pt-10 mt-10 border-t border-surface-container-high">
            <p className="text-xs text-on-surface-variant font-bold uppercase tracking-wider">{t('history.showingItems')}</p>
            <div className="flex gap-2">
              <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-surface-container-lowest ring-1 ring-outline-variant/30 text-on-surface-variant hover:text-primary transition-colors">
                &lt;
              </button>
              <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-primary text-white text-sm font-bold">1</button>
              <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-surface-container-lowest ring-1 ring-outline-variant/30 text-on-surface hover:bg-surface-container text-sm font-bold">2</button>
              <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-surface-container-lowest ring-1 ring-outline-variant/30 text-on-surface hover:bg-surface-container text-sm font-bold">3</button>
              <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-surface-container-lowest ring-1 ring-outline-variant/30 text-on-surface-variant hover:text-primary transition-colors">
                &gt;
              </button>
            </div>
          </div>
        </>
      ) : (
        <>
          <div className="space-y-6">
            {translations.map((trans) => (
            <article 
              key={trans.id} 
              className={`group relative rounded-xl p-6 shadow-sm transition-all flex flex-col md:flex-row gap-8 items-start border ${
                trans.highlight 
                  ? 'bg-gradient-to-br from-primary/5 to-white border-primary/20' 
                  : 'bg-surface-container-lowest border-outline-variant/20 hover:shadow-md'
              }`}
            >
              <div className="flex-1 space-y-4">
                <div className="flex items-center gap-3">
                  <span className={`inline-flex items-center px-2.5 py-1 rounded-md text-[10px] font-bold tracking-wider uppercase ${trans.highlight ? 'bg-primary text-white' : 'bg-primary/10 text-primary'}`}>
                    {trans.sourceLang}
                  </span>
                  <span className="text-outline text-sm">→</span>
                  <span className="inline-flex items-center px-2.5 py-1 rounded-md text-[10px] font-bold bg-surface-container text-on-surface-variant tracking-wider uppercase">
                    {trans.targetLang}
                  </span>
                </div>
                <div className="space-y-3">
                  <p className="text-lg font-bold text-on-surface leading-snug">{trans.sourceText}</p>
                  <p className="text-lg font-medium text-primary leading-snug">{trans.targetText}</p>
                </div>
              </div>
              <div className="shrink-0 flex flex-col items-end gap-3">
                <span className="text-xs font-semibold text-on-surface-variant flex items-center gap-1.5">
                  <Clock size={14} />
                  {trans.time}
                </span>
                <div className="flex gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
                  <button className="p-2 rounded-lg bg-surface-container hover:bg-primary-container hover:text-white transition-colors text-on-surface-variant">
                    <Copy size={16} />
                  </button>
                  <button className="p-2 rounded-lg bg-surface-container hover:bg-error/10 hover:text-error transition-colors text-on-surface-variant">
                    <Trash2 size={16} />
                  </button>
                </div>
              </div>
            </article>
          ))}
        </div>
        
        <div className="flex items-center justify-between pt-10 mt-10 border-t border-surface-container-high">
          <p className="text-xs text-on-surface-variant font-bold uppercase tracking-wider">{t('history.showingItems')}</p>
          <div className="flex gap-2">
            <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-surface-container-lowest ring-1 ring-outline-variant/30 text-on-surface-variant hover:text-primary transition-colors">
              &lt;
            </button>
            <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-primary text-white text-sm font-bold">1</button>
            <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-surface-container-lowest ring-1 ring-outline-variant/30 text-on-surface hover:bg-surface-container text-sm font-bold">2</button>
            <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-surface-container-lowest ring-1 ring-outline-variant/30 text-on-surface hover:bg-surface-container text-sm font-bold">3</button>
            <button className="w-9 h-9 flex items-center justify-center rounded-lg bg-surface-container-lowest ring-1 ring-outline-variant/30 text-on-surface-variant hover:text-primary transition-colors">
              &gt;
            </button>
          </div>
        </div>
        </>
      )}
    </div>
  );
}
