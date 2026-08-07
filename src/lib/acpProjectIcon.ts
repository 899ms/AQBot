export type AcpProjectIconType = 'emoji' | 'url' | 'file' | 'model_icon';

export interface AcpProjectIcon {
  type: AcpProjectIconType | string;
  value: string;
}

export const ACP_PROJECT_ICON_KEY = (id: string) => `aqbot_acp_project_icon_${id}`;

export function getAcpProjectIcon(projectId: string): AcpProjectIcon | null {
  try {
    const stored = localStorage.getItem(ACP_PROJECT_ICON_KEY(projectId));
    if (!stored) return null;
    return JSON.parse(stored) as AcpProjectIcon;
  } catch {
    return null;
  }
}

export function setAcpProjectIcon(projectId: string, icon: AcpProjectIcon | null): void {
  try {
    if (!icon || !icon.type || !icon.value) {
      localStorage.removeItem(ACP_PROJECT_ICON_KEY(projectId));
      return;
    }
    localStorage.setItem(ACP_PROJECT_ICON_KEY(projectId), JSON.stringify(icon));
  } catch {
    // ignore storage errors
  }
}
