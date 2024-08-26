import { AnimatePresence } from 'framer-motion';

import { EarningsContainer, EarningsWrapper } from './Earnings.styles.ts';
import formatBalance from '@app/utils/formatBalance.ts';
import { useCallback } from 'react';

import { useMiningStore } from '@app/store/useMiningStore.ts';
import CharSpinner from '@app/components/CharSpinner/CharSpinner.tsx';

const variants = {
    visible: {
        opacity: 1,
        y: -200,
        scale: 1.05,
        transition: {
            duration: 3,
            scale: {
                duration: 0.5,
            },
        },
    },
    hidden: {
        opacity: 0,
        y: -150,
        transition: { duration: 0.2, delay: 0.8 },
    },
};

export default function Earnings() {
    const earnings = useMiningStore((s) => s.earnings);
    const setEarnings = useMiningStore((s) => s.setEarnings);
    const setPostBlockAnimation = useMiningStore((s) => s.setPostBlockAnimation);
    const formatted = formatBalance(earnings || 0);
    const handleComplete = useCallback(() => {
        setEarnings(undefined);
        setPostBlockAnimation(true);
    }, [setEarnings, setPostBlockAnimation]);

    return (
        <EarningsContainer>
            <AnimatePresence>
                {earnings ? (
                    <EarningsWrapper
                        initial="hidden"
                        variants={variants}
                        animate="visible"
                        exit="hidden"
                        onAnimationComplete={() => {
                            handleComplete();
                        }}
                    >
                        <span>YOUR REWARD IS</span>
                        <CharSpinner value={formatted.toString()} fontSize={75} />
                    </EarningsWrapper>
                ) : null}
            </AnimatePresence>
        </EarningsContainer>
    );
}