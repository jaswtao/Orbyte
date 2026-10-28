/** 统一工作台、组件库和内容区的外观。Author: jaswtao · Date: 2026-10-15 */
import type { GlobalThemeOverrides } from 'naive-ui'

export type AppearanceMode = 'light' | 'dark'

export const uiFont = "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Microsoft YaHei', sans-serif"
export const palettes = {
  light: {
    background: '#F5F5F7', surface: '#EEEEF0', elevated: '#FFFFFF', content: '#FFFFFF',
    text: '#1D1D1F', muted: '#6E6E73', border: '#DADADD', accent: '#007AFF',
    accentHover: '#0066D6', selection: '#007AFF18', hover: '#00000006',
    terminalSelection: '#007AFF30', danger: '#C93432', warning: '#956000', success: '#248245',
  },
  dark: {
    background: '#1C1C1E', surface: '#242426', elevated: '#303033', content: '#1C1C1E',
    text: '#F5F5F7', muted: '#A1A1A6', border: '#3A3A3C', accent: '#0A84FF',
    accentHover: '#409CFF', selection: '#0A84FF28', hover: '#FFFFFF0A',
    terminalSelection: '#0A84FF55', danger: '#FF6961', warning: '#E9BA61', success: '#70C78C',
  },
} as const

export function resolveAppearance(preference: string | undefined, system: string | null): AppearanceMode {
  return preference === 'light' || (preference === 'system' && system === 'light') ? 'light' : 'dark'
}

export function workspaceBorderColor(mode: AppearanceMode, color: string | null = null, intensity = 100): string {
  const accent = color ?? palettes[mode].accent
  if (intensity === 100) return accent
  if (intensity === 0) return palettes[mode].border
  return `color-mix(in srgb, ${accent} ${intensity}%, ${palettes[mode].border})`
}

export function appearanceVariables(mode: AppearanceMode, borderColor: string | null = null, borderIntensity = 100) {
  return {
    ...Object.fromEntries(Object.entries(palettes[mode]).map(([key, value]) => [`--${key.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`, value])),
    '--workspace-border': workspaceBorderColor(mode, borderColor, borderIntensity),
  }
}

export function componentTheme(mode: AppearanceMode): GlobalThemeOverrides {
  const p = palettes[mode]
  return {
    common: {
      fontFamily: uiFont, fontSize: '13px', borderRadius: '8px',
      primaryColor: p.accent, primaryColorHover: p.accentHover, primaryColorPressed: p.accentHover,
      primaryColorSuppl: p.accent, bodyColor: p.background, cardColor: p.elevated,
      modalColor: p.elevated, popoverColor: p.elevated, inputColor: p.content,
      textColorBase: p.text, textColor1: p.text, textColor2: p.text, textColor3: p.muted,
      placeholderColor: p.muted, borderColor: p.border, dividerColor: p.border,
      errorColor: p.danger, warningColor: p.warning, successColor: p.success,
      heightMedium: '32px', heightSmall: '28px',
    },
    Button: {
      textColorPrimary: '#FFFFFF', textColorHoverPrimary: '#FFFFFF',
      textColorPressedPrimary: '#FFFFFF', textColorFocusPrimary: '#FFFFFF',
      fontWeight: '500', borderRadiusMedium: '8px', borderRadiusSmall: '7px',
    },
    Card: { borderRadius: '16px', paddingMedium: '24px', titleFontSizeMedium: '17px', titleFontWeight: '600' },
    Input: { color: p.content, colorFocus: p.content, borderFocus: `1px solid ${p.accent}`, boxShadowFocus: `0 0 0 3px ${p.selection}` },
    Dropdown: { borderRadius: '10px', optionColorHover: p.selection, optionTextColor: p.text },
  }
}

export function terminalAppearance(mode: AppearanceMode) {
  const p = palettes[mode]
  // 浅色终端使用更深的 ANSI 颜色，避免默认亮绿、亮黄在白底上难以阅读。
  const ansi = mode === 'light'
    ? {
      black: '#1D1D1F', red: '#B3261E', green: '#237B3A', yellow: '#875900',
      blue: '#0066CC', magenta: '#9B2393', cyan: '#006D77', white: '#737378',
      brightBlack: '#6E6E73', brightRed: '#C93432', brightGreen: '#248245', brightYellow: '#956000',
      brightBlue: '#0071E3', brightMagenta: '#A42DA0', brightCyan: '#007D89', brightWhite: '#1D1D1F',
    }
    : {
      black: '#48484A', red: '#FF6961', green: '#70C78C', yellow: '#E9BA61',
      blue: '#64AAFF', magenta: '#D799E6', cyan: '#6CCBD0', white: '#D1D1D6',
      brightBlack: '#8E8E93', brightRed: '#FF938A', brightGreen: '#98DCA5', brightYellow: '#F4D18C',
      brightBlue: '#91C3FF', brightMagenta: '#E4B4EF', brightCyan: '#9ADDE0', brightWhite: '#F5F5F7',
    }
  return { ...ansi, background: p.content, foreground: p.text, cursor: p.text, cursorAccent: p.content, selectionBackground: p.terminalSelection }
}
