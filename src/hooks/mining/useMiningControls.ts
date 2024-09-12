import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { setAnimationState } from '@app/visuals.ts';
import { useAppStateStore } from '@app/store/appStateStore.ts';

export function useMiningControls() {
    const setError = useAppStateStore((s) => s.setError);
    const handleStart = useCallback(async () => {
        console.info('Mining starting....');
        try {
            await invoke('start_mining', {})
                .then(async () => {
                    console.info('Mining started.');
                    setAnimationState('start');
                })
                .catch((e) => {
                    console.error(e);
                    // if there was a problem starting
                    setAnimationState('stop');
                });
        } catch (e) {
            console.error(e);
            const error = e as string;
            // if there was a problem starting
            setAnimationState('stop');
            setError(error);
        }
    }, [setError]);

    const handleStop = useCallback(
        async (args?: { isPause?: boolean }) => {
            console.info('Mining stopping...');
            try {
                await invoke('stop_mining', {})
                    .then(async () => {
                        setAnimationState(args?.isPause ? 'pause' : 'stop');

                        if (args?.isPause) {
                            console.info('Mining stopped, as pause, to be restarted.');
                        } else {
                            console.info('Mining stopped.');
                        }
                        // setAnimationState(args?.isPause ? 'pause' : 'stop');
                    })
                    .catch((e) => {
                        console.error(e);
                        // if there was a problem stopping
                        setAnimationState('start');
                    });
            } catch (e) {
                console.error(e);
                const error = e as string;
                // if there was a problem stopping
                setAnimationState('start');
                setError(error);
            }
        },
        [setError]
    );

    return { handleStart, handleStop };
}
