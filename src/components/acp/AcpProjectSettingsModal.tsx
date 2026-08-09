import { useEffect, useState } from 'react';
import { App, Avatar, Button, Input, Modal, theme } from 'antd';
import { FolderOpen } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { IconEditor } from '@/components/shared/IconEditor';
import {
  getAcpProjectIcon,
  setAcpProjectIcon,
  type AcpProjectIconType,
} from '@/lib/acpProjectIcon';
import { useAcpStore } from '@/stores/acpStore';
import type { AcpProject } from '@/types/acp';

interface AcpProjectSettingsModalProps {
  open: boolean;
  project: AcpProject | null;
  onClose: () => void;
}

/**
 * Project settings — layout mirrors ConversationSettingsModal:
 * centered icon editor, then labeled fields (name, directory).
 */
export function AcpProjectSettingsModal({
  open,
  project,
  onClose,
}: AcpProjectSettingsModalProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const { message } = App.useApp();
  const updateProject = useAcpStore((s) => s.updateProject);

  const [name, setName] = useState('');
  const [rootPath, setRootPath] = useState('');
  const [iconType, setIconType] = useState<string | null>(null);
  const [iconValue, setIconValue] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open || !project) return;
    setName(project.name);
    setRootPath(project.root_path);
    const icon = getAcpProjectIcon(project.id);
    setIconType(icon?.type ?? null);
    setIconValue(icon?.value ?? null);
  }, [open, project]);

  if (!project) return null;

  const labelStyle: React.CSSProperties = {
    fontSize: 13,
    color: token.colorText,
    marginBottom: 6,
    display: 'flex',
    alignItems: 'center',
    gap: 4,
  };

  const handlePickDirectory = async () => {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: rootPath || undefined,
    });
    if (!selected || Array.isArray(selected)) return;
    setRootPath(selected);
  };

  const handleSave = async () => {
    const trimmedName = name.trim();
    const trimmedPath = rootPath.trim();
    if (!trimmedName) {
      message.warning(t('agentPage.projectNameRequired'));
      return;
    }
    if (!trimmedPath) {
      message.warning(t('agentPage.projectPathRequired'));
      return;
    }
    setSaving(true);
    try {
      await updateProject(project.id, {
        name: trimmedName,
        rootPath: trimmedPath,
      });
      if (iconType && iconValue) {
        setAcpProjectIcon(project.id, {
          type: iconType as AcpProjectIconType,
          value: iconValue,
        });
      } else {
        setAcpProjectIcon(project.id, null);
      }
      // Bump projects array so sidebar re-reads icon from localStorage
      useAcpStore.setState((s) => ({ projects: [...s.projects] }));
      message.success(t('common.saveSuccess'));
      onClose();
    } catch (e) {
      message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title={t('agentPage.projectSettings')}
      open={open}
      mask={{ enabled: true, blur: true }}
      onCancel={onClose}
      width={520}
      destroyOnHidden
      footer={
        <div style={{ display: 'flex', justifyContent: 'space-between' }}>
          <Button onClick={onClose}>{t('common.cancel')}</Button>
          <Button type="primary" onClick={() => void handleSave()} loading={saving}>
            {t('common.save')}
          </Button>
        </div>
      }
    >
      <div data-os-scrollbar style={{ maxHeight: '70vh', overflowY: 'auto', paddingRight: 4 }}>
        <div style={{ display: 'flex', justifyContent: 'center', margin: '8px 0 16px' }}>
          <IconEditor
            iconType={iconType}
            iconValue={iconValue}
            onChange={(type, value) => {
              setIconType(type);
              setIconValue(value);
            }}
            size={64}
            defaultIcon={
              <Avatar
                size={64}
                icon={<FolderOpen size={28} />}
                style={{
                  backgroundColor: token.colorFillSecondary,
                  color: token.colorTextSecondary,
                }}
              />
            }
            showClear={!!(iconType && iconValue)}
            showModelIcons
          />
        </div>

        <div style={{ marginBottom: 16 }}>
          <div style={labelStyle}>{t('agentPage.projectName')}</div>
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t('agentPage.projectName')}
          />
        </div>

        <div style={{ marginBottom: 8 }}>
          <div style={labelStyle}>{t('agentPage.projectDirectory')}</div>
          <div style={{ display: 'flex', gap: 8 }}>
            <Input
              value={rootPath}
              onChange={(e) => setRootPath(e.target.value)}
              placeholder={t('agentPage.projectDirectory')}
              style={{ flex: 1 }}
            />
            <Button icon={<FolderOpen size={14} />} onClick={() => void handlePickDirectory()}>
              {t('agentPage.browseDirectory')}
            </Button>
          </div>
        </div>
      </div>
    </Modal>
  );
}
