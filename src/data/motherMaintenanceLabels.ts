// Shared by translations and send-time detection so a language change keeps the workflow.
export const motherMaintenanceLabels = {
  residuals: {
    en: 'Analyze/clean leftovers after uninstall',
    'zh-Hans': '分析/清理无法被卸载的残留',
    'zh-Hant': '分析/清理無法被解除安裝的殘留',
    ja: 'アンインストール後の残留ファイルを分析・整理',
  },
  diskSpace: {
    en: 'Analyze disk space usage',
    'zh-Hans': '分析磁盘空间占用',
    'zh-Hant': '分析磁碟空間占用',
    ja: 'ディスク容量の使用状況を分析',
  },
} as const;
