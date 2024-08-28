import { useAirdropStore } from "@app/store/useAirdropStore";
// import { useInterval } from "../useInterval";
import { useCallback, useEffect } from "react";
import { invoke } from '@tauri-apps/api/tauri';

export function useAirdropTokensRefresh() {
    const { airdropTokens, setAirdropTokens } = useAirdropStore();

    // Handle refreshing the access token
    const handleRefresh = useCallback(() => {
        const expired = airdropTokens?.expiresAt && new Date(airdropTokens?.expiresAt * 1000) > new Date();
        if (airdropTokens && expired) {
            fetch('https://airdrop.tari.com/api/auth/local/refresh', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    refreshToken: airdropTokens.refreshToken
                })
            })
                .then(response => response.json())
                .then((data) => {
                    setAirdropTokens(data);
                });
        }
    }, [airdropTokens, setAirdropTokens]);

    useEffect(() => {
        const interval = setInterval(handleRefresh, 1000 * 60 * 60);
        return () => clearInterval(interval);
    }, [handleRefresh]);

    // Handle setting the access token
    useEffect(() => {
        if (!airdropTokens) return;
        invoke('set_airdrop_access_token', {token: airdropTokens?.token})
            .catch((error) => {
                console.error('Error getting airdrop tokens', error);
            });
    }, [airdropTokens]);
}
