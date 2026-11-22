import { detectLocale, LOCALE_STORAGE_KEY, resolveLocale, type Locale } from '../i18n/locales';

// Keep these strings available even if a language pack fails to initialize.

interface ErrorStrings {
  title: string;
  body: string;
  copy: string;
  reload: string;
  bootTitle: string;
}

export const ERROR_MESSAGES: Record<Locale, ErrorStrings> = {
  en: {
    title: 'Something went wrong',
    body: 'The app hit an unexpected error. Copy the details below, send them to us, then reload.',
    copy: 'Copy error',
    reload: 'Reload',
    bootTitle: 'Startup failed',
  },
  'zh-Hans': {
    title: '应用出错了',
    body: '界面遇到一个未处理的错误。复制下面的错误信息发给我们,然后重新加载即可。',
    copy: '复制错误信息',
    reload: '重新加载',
    bootTitle: '启动失败',
  },
  'zh-Hant': {
    title: '應用程式發生錯誤',
    body: '介面發生未處理的錯誤。請複製下方的錯誤資訊傳給我們,然後重新載入。',
    copy: '複製錯誤資訊',
    reload: '重新載入',
    bootTitle: '啟動失敗',
  },
  ja: {
    title: 'エラーが発生しました',
    body: '画面で予期しないエラーが発生しました。下のエラー情報をコピーして送信し、再読み込みしてください。',
    copy: 'エラーをコピー',
    reload: '再読み込み',
    bootTitle: '起動に失敗しました',
  },
  ko: {
    title: '문제가 발생했습니다',
    body: '예기치 않은 오류가 발생했습니다. 아래 오류 정보를 복사해 보내 주신 뒤 다시 불러오세요.',
    copy: '오류 복사',
    reload: '다시 불러오기',
    bootTitle: '시작 실패',
  },
  es: {
    title: 'Se ha producido un error',
    body: 'La aplicación encontró un error inesperado. Copia los detalles, envíanoslos y vuelve a cargar la aplicación.',
    copy: 'Copiar error',
    reload: 'Volver a cargar',
    bootTitle: 'Error al iniciar',
  },
  fr: {
    title: 'Une erreur est survenue',
    body: 'Une erreur inattendue est survenue. Copiez les détails ci-dessous, envoyez-les-nous, puis rechargez l’application.',
    copy: 'Copier l’erreur',
    reload: 'Recharger',
    bootTitle: 'Échec du démarrage',
  },
  de: {
    title: 'Ein Fehler ist aufgetreten',
    body: 'Ein unerwarteter Fehler ist aufgetreten. Kopieren Sie die folgenden Details, senden Sie sie uns und laden Sie die App neu.',
    copy: 'Fehler kopieren',
    reload: 'Neu laden',
    bootTitle: 'Start fehlgeschlagen',
  },
  pt: {
    title: 'Ocorreu um erro',
    body: 'A aplicação encontrou um erro inesperado. Copie os detalhes abaixo, envie-os para nós e recarregue a aplicação.',
    copy: 'Copiar erro',
    reload: 'Recarregar',
    bootTitle: 'Falha ao iniciar',
  },
  ru: {
    title: 'Произошла ошибка',
    body: 'Возникла непредвиденная ошибка. Скопируйте сведения об ошибке, отправьте их нам и перезагрузите приложение.',
    copy: 'Копировать ошибку',
    reload: 'Перезагрузить',
    bootTitle: 'Ошибка запуска',
  },
  vi: {
    title: 'Đã xảy ra lỗi',
    body: 'Ứng dụng gặp lỗi ngoài dự kiến. Hãy sao chép thông tin lỗi bên dưới, gửi cho chúng tôi rồi tải lại ứng dụng.',
    copy: 'Sao chép lỗi',
    reload: 'Tải lại',
    bootTitle: 'Không thể khởi động',
  },
};

export function pickErrorLang(): Locale {
  try {
    const stored = localStorage.getItem(LOCALE_STORAGE_KEY);
    if (stored) return resolveLocale(stored);
  } catch {
    // Storage can be unavailable during startup failures.
  }
  const tag = typeof document !== 'undefined' ? document.documentElement.lang : '';
  return tag && tag !== 'en' ? resolveLocale(tag) : detectLocale();
}
