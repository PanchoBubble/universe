import { m } from 'framer-motion';
import styled from 'styled-components';

export const Wrapper = styled('div')`
    position: fixed;
    width: 100%;
    height: 100%;
    top: 0;
    left: 0;
    z-index: 99999;

    display: flex;
    justify-content: center;
    align-items: center;

    pointer-events: all;
`;

export const Cover = styled(m.div)`
    position: absolute;
    width: 100%;
    height: 100%;
    top: 0;
    left: 0;
    background-color: rgba(0, 0, 0, 0.5);
    z-index: 0;
    cursor: pointer;
`;

export const BoxWrapper = styled(m.div)`
    width: 100%;
    max-width: 635px;
    flex-shrink: 0;

    border-radius: 35px;
    background: linear-gradient(180deg, #c9eb00 32.79%, #fff 92.04%);
    box-shadow: 28px 28px 77px 0px rgba(0, 0, 0, 0.1);

    position: relative;
    z-index: 1;

    padding: 180px 50px 42px 50px;

    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 40px;
`;

export const CloseButton = styled('button')`
    position: absolute;
    top: 0;
    left: 100%;
    margin-left: 18px;

    transition: transform 0.2s ease;

    &:hover {
        transform: scale(1.1);
    }
`;