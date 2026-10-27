/** 侧栏菜单计算。Author: jaswtao · Date: 2026-10-14 */
import type { Project, Settings } from '../projectApi'
import { setLocale, t } from '../../i18n/translate'

export { setLocale }

export function buildMenuOptions(target: { kind: 'group' | 'project'; id: string } | null, projects: Project[], settings: Settings | null, disabled: boolean) {
  if (!target) return []
  const options = target.kind === 'group'
    ? [{ key: 'add-project', label: t('sidebar.addProject') }, { key: 'rename', label: t('sidebar.renameGroup') }, { key: 'delete', label: t('sidebar.deleteGroup') }]
    : [
        { key: 'toggle-header', label: settings?.show_workspace_header ? t('sidebar.hideHeader') : t('sidebar.showHeader') },
        { key: 'edit', label: t('sidebar.editProject') },
        { key: 'favorite', label: projects.find((p) => p.id === target.id)?.favorite ? t('sidebar.unfavorite') : t('sidebar.favorite') },
        { key: 'hidden', label: projects.find((p) => p.id === target.id)?.hidden ? t('sidebar.unhideProject') : t('sidebar.hideProject') },
        { key: 'delete', label: t('sidebar.deleteProject') },
      ]
  return options.map((option) => ({ ...option, disabled }))
}
