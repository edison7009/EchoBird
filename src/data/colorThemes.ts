export type ColorThemeId =
  | 'oatgray'
  | 'terracotta'
  | 'aurora'
  | 'forest'
  | 'twilight'
  | 'rosewood'
  | 'oceanic'
  | 'cobalt'
  | 'saffron'
  | 'jade'
  | 'indigo'
  | 'ember'
  | 'lavender'
  | 'sage'
  | 'inkwash'
  | 'sakura'
  | 'copper'
  | 'midnight';

export interface ColorThemeVariant {
  shell: string;
  canvas: string;
  secondary: string;
  tertiary: string;
  input: string;
}
export interface ColorTheme {
  id: ColorThemeId;
  labelZh: string;
  labelEn: string;
  labelJa: string;
  light: ColorThemeVariant;
  dark: ColorThemeVariant;
}

export const COLOR_THEMES: ColorTheme[] = [
  {
    id: 'oatgray',
    labelZh: '燕麦暖灰',
    labelEn: 'Oat gray',
    labelJa: 'オートグレー',
    light: {
      shell: '#F7F7F7',
      canvas: '#FFFFFF',
      secondary: '#F2F2F2',
      tertiary: '#E5E5E5',
      input: '#F5F5F5',
    },
    dark: {
      shell: '#202120',
      canvas: '#1C1D1C',
      secondary: '#252625',
      tertiary: '#36322C',
      input: '#303130',
    },
  },
  {
    id: 'terracotta',
    labelZh: '陶土',
    labelEn: 'Terracotta',
    labelJa: 'テラコッタ',
    light: {
      shell: '#F2E3DA',
      canvas: '#FAF3EE',
      secondary: '#F5E7DF',
      tertiary: '#E8C6B6',
      input: '#F2E0D7',
    },
    dark: {
      shell: '#302321',
      canvas: '#241918',
      secondary: '#3A2926',
      tertiary: '#5A3932',
      input: '#46302B',
    },
  },
  {
    id: 'aurora',
    labelZh: '极光',
    labelEn: 'Aurora',
    labelJa: 'オーロラ',
    light: {
      shell: '#E4F0EE',
      canvas: '#F3F9F7',
      secondary: '#E8F4F1',
      tertiary: '#B9DCD6',
      input: '#DFEFEC',
    },
    dark: {
      shell: '#172D31',
      canvas: '#102327',
      secondary: '#1B373B',
      tertiary: '#28555A',
      input: '#214449',
    },
  },
  {
    id: 'forest',
    labelZh: '苔原',
    labelEn: 'Moss',
    labelJa: 'モス',
    light: {
      shell: '#E8EBDD',
      canvas: '#F5F7ED',
      secondary: '#E8EEDB',
      tertiary: '#C7D3AE',
      input: '#E1E9D2',
    },
    dark: {
      shell: '#202A24',
      canvas: '#17211B',
      secondary: '#263329',
      tertiary: '#3A4C3B',
      input: '#304034',
    },
  },
  {
    id: 'twilight',
    labelZh: '暮紫',
    labelEn: 'Twilight',
    labelJa: 'トワイライト',
    light: {
      shell: '#E8E5F0',
      canvas: '#F7F5FB',
      secondary: '#ECE8F4',
      tertiary: '#CDC4E0',
      input: '#E6E1F0',
    },
    dark: {
      shell: '#252238',
      canvas: '#19172A',
      secondary: '#2D2944',
      tertiary: '#4B4268',
      input: '#393250',
    },
  },
  {
    id: 'rosewood',
    labelZh: '玫瑰木',
    labelEn: 'Rosewood',
    labelJa: 'ローズウッド',
    light: {
      shell: '#F0E1E3',
      canvas: '#FAF4F4',
      secondary: '#F2E6E8',
      tertiary: '#DDBDC3',
      input: '#EEDEE1',
    },
    dark: {
      shell: '#312126',
      canvas: '#24171C',
      secondary: '#3C252D',
      tertiary: '#5B3742',
      input: '#4A2D36',
    },
  },
  {
    id: 'oceanic',
    labelZh: '潮汐',
    labelEn: 'Tidal',
    labelJa: 'タイダル',
    light: {
      shell: '#DFEDF1',
      canvas: '#F2F9FA',
      secondary: '#E3F1F3',
      tertiary: '#B6D8DE',
      input: '#DAECEF',
    },
    dark: {
      shell: '#172A33',
      canvas: '#102029',
      secondary: '#1D3540',
      tertiary: '#2D5662',
      input: '#254652',
    },
  },
  {
    id: 'cobalt',
    labelZh: '钴蓝',
    labelEn: 'Cobalt',
    labelJa: 'コバルト',
    light: {
      shell: '#E2E8F5',
      canvas: '#F4F7FD',
      secondary: '#E7EDFA',
      tertiary: '#BFCDE8',
      input: '#DFE7F6',
    },
    dark: {
      shell: '#18243A',
      canvas: '#101A2C',
      secondary: '#213352',
      tertiary: '#304F80',
      input: '#29436C',
    },
  },
  {
    id: 'saffron',
    labelZh: '藏红花',
    labelEn: 'Saffron',
    labelJa: 'サフラン',
    light: {
      shell: '#F5E7C7',
      canvas: '#FCF7E8',
      secondary: '#F6EDC9',
      tertiary: '#E7C96D',
      input: '#F3E6B7',
    },
    dark: {
      shell: '#33291B',
      canvas: '#241D12',
      secondary: '#44351B',
      tertiary: '#6D5421',
      input: '#51401C',
    },
  },
  {
    id: 'jade',
    labelZh: '玉髓',
    labelEn: 'Jade',
    labelJa: 'ジェイド',
    light: {
      shell: '#DDEDE6',
      canvas: '#F1FAF5',
      secondary: '#E3F2EA',
      tertiary: '#A9D2BE',
      input: '#D7ECE1',
    },
    dark: {
      shell: '#172B27',
      canvas: '#10201D',
      secondary: '#203B35',
      tertiary: '#2E5C4D',
      input: '#285044',
    },
  },
  {
    id: 'indigo',
    labelZh: '靛青',
    labelEn: 'Indigo',
    labelJa: 'インディゴ',
    light: {
      shell: '#E4E5F4',
      canvas: '#F4F5FC',
      secondary: '#E9EAF8',
      tertiary: '#B9BCE0',
      input: '#DFE1F3',
    },
    dark: {
      shell: '#1C2038',
      canvas: '#12162A',
      secondary: '#252C4B',
      tertiary: '#3D4E7A',
      input: '#303E63',
    },
  },
  {
    id: 'ember',
    labelZh: '余烬',
    labelEn: 'Ember',
    labelJa: 'エンバー',
    light: {
      shell: '#F3DFD2',
      canvas: '#FCF3ED',
      secondary: '#F4E4D9',
      tertiary: '#E2AD91',
      input: '#F0D9CB',
    },
    dark: {
      shell: '#35211D',
      canvas: '#241513',
      secondary: '#462923',
      tertiary: '#704033',
      input: '#583128',
    },
  },
  {
    id: 'lavender',
    labelZh: '鸢尾',
    labelEn: 'Iris',
    labelJa: 'アイリス',
    light: {
      shell: '#EAE2F4',
      canvas: '#F8F4FD',
      secondary: '#EEE7F8',
      tertiary: '#CDB8E2',
      input: '#E7DEF4',
    },
    dark: {
      shell: '#292239',
      canvas: '#1B172A',
      secondary: '#352B4A',
      tertiary: '#594375',
      input: '#45345F',
    },
  },
  {
    id: 'sage',
    labelZh: '鼠尾草',
    labelEn: 'Sage',
    labelJa: 'セージ',
    light: {
      shell: '#E5E9DE',
      canvas: '#F4F7EE',
      secondary: '#E9EFDF',
      tertiary: '#C0D0AE',
      input: '#E1E9D5',
    },
    dark: {
      shell: '#242B24',
      canvas: '#181F19',
      secondary: '#2C382C',
      tertiary: '#4A6248',
      input: '#3A503A',
    },
  },
  {
    id: 'inkwash',
    labelZh: '水墨',
    labelEn: 'Ink wash',
    labelJa: '墨絵',
    light: {
      shell: '#E6E5E1',
      canvas: '#F5F4F0',
      secondary: '#EAE9E4',
      tertiary: '#C7C5BD',
      input: '#E3E2DC',
    },
    dark: {
      shell: '#202124',
      canvas: '#151619',
      secondary: '#2B2D31',
      tertiary: '#45484E',
      input: '#383B40',
    },
  },
  {
    id: 'sakura',
    labelZh: '樱雾',
    labelEn: 'Sakura mist',
    labelJa: '桜ミスト',
    light: {
      shell: '#F3E1E6',
      canvas: '#FCF4F7',
      secondary: '#F5E7EC',
      tertiary: '#E1B8C5',
      input: '#F1DEE4',
    },
    dark: {
      shell: '#33222A',
      canvas: '#24171E',
      secondary: '#432B36',
      tertiary: '#684151',
      input: '#503342',
    },
  },
  {
    id: 'copper',
    labelZh: '铜绿',
    labelEn: 'Verdigris',
    labelJa: 'ベルディグリ',
    light: {
      shell: '#DDEBE5',
      canvas: '#F0F8F3',
      secondary: '#E2F0E8',
      tertiary: '#A8CDBB',
      input: '#D6E9DF',
    },
    dark: {
      shell: '#1B2C2B',
      canvas: '#11201F',
      secondary: '#26403B',
      tertiary: '#3B685C',
      input: '#2F554D',
    },
  },
  {
    id: 'midnight',
    labelZh: '午夜蓝',
    labelEn: 'Midnight',
    labelJa: 'ミッドナイト',
    light: {
      shell: '#DEE7F0',
      canvas: '#F3F7FB',
      secondary: '#E4EDF5',
      tertiary: '#B6C9DB',
      input: '#DBE6F0',
    },
    dark: {
      shell: '#162337',
      canvas: '#0D1728',
      secondary: '#1B2E49',
      tertiary: '#2C4B70',
      input: '#223C5C',
    },
  },
];

export const isColorThemeId = (value: unknown): value is ColorThemeId =>
  COLOR_THEMES.some((theme) => theme.id === value);
export const getColorTheme = (id: ColorThemeId): ColorTheme =>
  COLOR_THEMES.find((theme) => theme.id === id) ?? COLOR_THEMES[0];
