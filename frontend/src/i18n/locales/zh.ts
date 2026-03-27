export default {
  translation: {
    sidebar: {
      general: "常规设置",
      history: "历史记录",
      ocr: "OCR 设置",
      ai: "AI 设置"
    },
    general: {
      systemLanguage: "系统语言",
      systemLanguageDesc: "选择应用程序界面的语言。",
      interfaceLanguage: "界面语言",
      appStartup: "开机启动",
      appStartupDesc: "确保 Flick 在您登录系统时已准备就绪。",
      launchAtStartup: "开机时启动",
      dataRetention: "数据保留",
      dataRetentionDesc: "通过限制缓存截图的数量来管理您的存储空间。",
      maxScreenshots: "最多保留截图数量",
      items: "项",
      storagePath: "存储路径",
      storagePathDesc: "查看 Flick 当前保存应用数据、系统设置和截图文件的位置。",
      screenshotSavePath: "截图保存路径",
      changePath: "更改路径",
      appDataDirectory: "应用数据目录",
      screenshotDirectory: "截图目录",
      globalHotkeys: "全局快捷键",
      globalHotkeysDesc: "配置无缝捕获工作流的快速操作触发器。",
      action: "操作",
      captureScreenshot: "捕获截图",
      screenshotTranslate: "截图并翻译",
      recordingShortcut: "正在录制...",
      pressShortcut: "请直接按下新的快捷键，按 Esc 取消。",
      shortcutModifierHint: "快捷键至少需要包含一个修饰键。",
      retentionHint: "保存新截图后，超出上限的旧截图会被自动删除。",
      saving: "正在保存...",
      discardChanges: "放弃更改",
      savePreferences: "保存首选项"
    },
    ocr: {
      enableShortcut: "启用 OCR 快捷键",
      autoTranslate: "自动翻译",
      discard: "放弃",
      saveChanges: "保存更改"
    },
    ai: {
      providerSelection: "提供商选择",
      apiConfig: "API 配置",
      modelSelection: "模型选择",
      apiAddress: "API 地址",
      apiKey: "API 密钥",
      defaultPrompt: "默认提示词",
      systemInstruction: "系统指令",
      advancedParameters: "高级参数",
      maxTokens: "最大 Token 数",
      temperature: "温度 (Temperature)",
      discard: "放弃",
      saveConfig: "保存配置"
    },
    history: {
      screenshotHistory: "截图历史",
      translationHistory: "翻译历史",
      view: "查看",
      preview: "预览",
      storageDirectory: "保存目录",
      notAvailable: "暂无可用目录",
      itemCount: "{{count}} 张截图",
      refresh: "刷新",
      loading: "正在加载截图历史...",
      loadFailed: "截图历史加载失败",
      emptyTitle: "还没有截图记录",
      emptyDesc: "完成一次截图后，这里会自动显示保存到本地目录中的历史截图。",
      copyPath: "复制路径",
      copyTranslation: "复制翻译",
      copied: "已复制",
      translationComingSoon: "翻译历史暂未接入",
      translationComingSoonDesc: "当前页面已接入真实截图历史，翻译历史后续再补。"
    },
    widget: {
      sourceText: "源文本",
      translation: "翻译结果",
      translate: "翻译",
      english: "英语",
      chinese: "中文"
    }
  }
};
