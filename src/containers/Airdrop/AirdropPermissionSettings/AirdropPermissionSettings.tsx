import { Switch } from '@mui/material';
import { BoxWrapper, Text, TextWrapper, Title, Wrapper } from './styles';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/tauri';
import { useUIStore } from '@app/store/useUIStore';

export default function AirdropPermissionSettings() {
    const telemetryMode = useUIStore((s) => s.telemetryMode);
    const toggleTelemetryMode = useUIStore((s) => s.toggleTelemetryMode);
    const { t } = useTranslation(['airdrop'], { useSuspense: false });

    const handleChange = () => {
        toggleTelemetryMode();
        invoke('set_telemetry_mode', { telemetryMode: !telemetryMode });
    };

    return (
        <Wrapper>
            <BoxWrapper>
                <TextWrapper>
                    <Title>{t('permission.title')}</Title>
                    <Text>{t('permission.text')}</Text>
                </TextWrapper>
                <Switch checked={telemetryMode} onChange={handleChange} color="primary" size="medium" />
            </BoxWrapper>
        </Wrapper>
    );
}
