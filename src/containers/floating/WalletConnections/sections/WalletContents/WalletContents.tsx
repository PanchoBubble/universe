import { ContentWrapper, WalletAddress } from '../../WalletConnections.style';
import { useDisconnect, useAppKit } from '@reown/appkit/react';
import MMFox from '../../icons/mm-fox';
import { useAccount, useBalance } from 'wagmi';
import { WalletButton } from '../../components/WalletButton/WalletButton';
import {
    ActiveDot,
    ConnectedWalletWrapper,
    StatusWrapper,
    WalletContentsContainer,
    WalletValue,
    WalletValueLeft,
    WalletValueRight,
} from './WalletContents.styles';
import { truncateMiddle } from '@app/utils/truncateString.ts';
import { useMemo } from 'react';
import { setWalletConnectModalStep } from '@app/store/actions/walletStoreActions';
import { SwapStep } from '@app/store';
import { getCurrencyIcon } from '../../helpers/getIcon';

export const WalletContents = () => {
    const { disconnect } = useDisconnect();
    const { open } = useAppKit();
    const dataAcc = useAccount();

    const { data: accountBalance } = useBalance({ address: dataAcc.address });

    const walletChainIcon = useMemo(() => {
        if (!accountBalance?.symbol) return null;
        const icon = getCurrencyIcon({
            simbol: accountBalance?.symbol,
            width: 20,
        });
        switch (accountBalance?.symbol.toLowerCase()) {
            case 'eth':
                return (
                    <WalletValueLeft>
                        {icon}
                        <span>{'Ethereum'}</span>
                    </WalletValueLeft>
                );

            case 'pol':
                return (
                    <WalletValueLeft>
                        {icon}
                        <span>{'Polygon'}</span>
                    </WalletValueLeft>
                );
            default:
                return null;
        }
    }, [accountBalance?.symbol]);

    const value = useMemo(() => {
        if (!accountBalance?.value) return 0;
        const factor = 10n ** BigInt(accountBalance.decimals);
        return (Number(accountBalance.value) / Number(factor)).toString();
    }, [accountBalance]);

    return (
        <WalletContentsContainer>
            <div>
                <WalletButton
                    variant="secondary"
                    size="small"
                    onClick={() =>
                        open({
                            view: 'Networks',
                        })
                    }
                >
                    {'Switch Network'}
                </WalletButton>
            </div>
            <ConnectedWalletWrapper>
                <WalletButton variant="error" onClick={() => disconnect()}>
                    {'Disconnect'}
                </WalletButton>
                <StatusWrapper>
                    <ActiveDot />
                    <MMFox width="25" />
                    <WalletAddress>{truncateMiddle(dataAcc.address || '', 4)}</WalletAddress>
                </StatusWrapper>
            </ConnectedWalletWrapper>
            <ContentWrapper>
                <WalletValue>
                    {walletChainIcon}
                    <WalletValueRight>
                        {value} {accountBalance?.symbol}
                    </WalletValueRight>
                </WalletValue>
            </ContentWrapper>

            <WalletButton variant="primary" onClick={() => setWalletConnectModalStep(SwapStep.Swap)} size="large">
                {'Continue'}
            </WalletButton>
        </WalletContentsContainer>
    );
};
